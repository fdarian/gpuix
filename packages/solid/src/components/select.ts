import {
  createComponent,
  createContext,
  createSignal,
  onCleanup,
  onMount,
  Show,
  useContext,
  type JSX,
} from "solid-js"
import type { HostMouseEvent, HostPointerEvent, HostProps, KeyEvent } from "@gpuix/native/host"
import type { HostElement } from "../host.js"
import { jsx } from "../jsx-runtime.js"
import { useGpuixRequired } from "../root.js"
import {
  FloatingLayer,
  floatingRootStyle,
  renderSlot,
  resolveStyle,
  type FloatingContentProps,
  type StateStyle,
  DismissableLayer,
} from "./floating.js"

export interface SelectItemData {
  value: string
  label?: JSX.Element
  textValue?: string
}

interface ItemRecord { value: string; disabled: boolean }
interface SelectContextValue {
  open(): boolean
  value(): string | undefined
  active(): string | null
  disabled: boolean
  labels: Map<string, JSX.Element>
  trigger: { current: HostElement | null }
  /** `restoreFocus: false` when the user pressed elsewhere, so the press keeps
   *  the focus it just gave. */
  setOpen(value: boolean, restoreFocus?: boolean): void
  setActive(value: string | null): void
  move(delta: number): void
  select(value: string): void
  register(item: ItemRecord): () => void
}

const SelectContext = createContext<SelectContextValue>()
function context(name: string): SelectContextValue {
  const value = useContext(SelectContext)
  if (!value) throw new Error(`${name} must be used inside Select`)
  return value
}

export interface SelectProps extends HostProps {
  children?: JSX.Element
  items?: readonly SelectItemData[]
  value?: string
  defaultValue?: string
  onValueChange?: (value: string) => void
  open?: boolean
  defaultOpen?: boolean
  onOpenChange?: (open: boolean) => void
  disabled?: boolean
}

export function Select(props: SelectProps): JSX.Element {
  const renderer = useGpuixRequired()
  const [internalValue, setInternalValue] = createSignal(props.defaultValue)
  const [internalOpen, setInternalOpen] = createSignal(props.defaultOpen ?? false)
  const [active, setActive] = createSignal<string | null>(null)
  const items: ItemRecord[] = []
  const trigger: { current: HostElement | null } = { current: null }
  const value = () => props.value === undefined ? internalValue() : props.value
  const open = () => props.open === undefined ? internalOpen() : props.open
  const setValue = (next: string) => {
    const previous = value()
    if (props.value === undefined) setInternalValue(next)
    if (previous !== next) props.onValueChange?.(next)
  }
  const setOpen = (next: boolean, restoreFocus = true) => {
    const previous = open()
    if (props.open === undefined) setInternalOpen(next)
    if (previous !== next) props.onOpenChange?.(next)
    if (!next && restoreFocus && trigger.current) renderer.focusElement?.(trigger.current.id)
    if (next) setActive(items.find((item) => item.value === value() && !item.disabled)?.value ?? null)
  }
  const move = (delta: number) => {
    const enabled = items.filter((item) => !item.disabled)
    if (enabled.length === 0) return
    const current = enabled.findIndex((item) => item.value === active())
    const start = current < 0 ? (delta > 0 ? -1 : 0) : current
    setActive(enabled[(start + delta + enabled.length) % enabled.length]!.value)
  }
  const select = (next: string) => {
    if (props.disabled || items.find((item) => item.value === next)?.disabled) return
    setValue(next)
    setOpen(false)
  }
  const labels = new Map<string, JSX.Element>()
  for (const item of props.items ?? []) labels.set(item.value, item.label ?? item.textValue ?? item.value)
  const valueContext: SelectContextValue = {
    open,
    value,
    active,
    disabled: props.disabled ?? false,
    labels,
    trigger,
    setOpen,
    setActive,
    move,
    select,
    register(item) {
      items.push(item)
      return () => {
        const index = items.indexOf(item)
        if (index >= 0) items.splice(index, 1)
      }
    },
  }
  return createComponent(SelectContext.Provider, {
    value: valueContext,
    get children() {
      return jsx("div", {
        ...props,
        style: floatingRootStyle(props.style),
        get children() { return props.children },
      })
    },
  })
}

export interface SelectTriggerState { open: boolean; disabled: boolean; placeholder: boolean }
export interface SelectTriggerProps extends Omit<HostProps, "style"> {
  children?: JSX.Element
  asChild?: boolean
  disabled?: boolean
  style?: StateStyle<SelectTriggerState>
  ref?: (element: HostElement) => void
}

export function SelectTrigger(props: SelectTriggerProps): JSX.Element {
  const state = context("SelectTrigger")
  const disabled = () => props.disabled ?? state.disabled
  return renderSlot({
    asChild: props.asChild,
    children: props.children,
    props: {
      ...props,
      ref(element: HostElement) {
        state.trigger.current = element
        props.ref?.(element)
      },
      get tabIndex() { return disabled() ? -1 : props.tabIndex },
      get style() {
        return resolveStyle(props.style, {
          open: state.open(),
          disabled: disabled(),
          placeholder: state.value() === undefined,
        })
      },
      onClick(event: HostPointerEvent) {
        props.onClick?.(event)
        if (!disabled()) state.setOpen(!state.open())
      },
      onKeyDown(event: KeyEvent) {
        props.onKeyDown?.(event)
        if (disabled()) return
        if (event.key === "down") state.open() ? state.move(1) : state.setOpen(true)
        else if (event.key === "up") state.open() ? state.move(-1) : state.setOpen(true)
        else if (event.key === "enter" || event.key === "space") state.setOpen(!state.open())
      },
    },
    defaultTabIndex: 0,
  })
}

export interface SelectValueProps extends HostProps { children?: JSX.Element; placeholder?: JSX.Element }
export function SelectValue(props: SelectValueProps): JSX.Element {
  const state = context("SelectValue")
  return jsx("div", {
    ...props,
    get children() {
      const selected = state.value()
      return props.children ?? (selected === undefined ? props.placeholder : (state.labels.get(selected) ?? selected))
    },
    })
}

export interface SelectContentProps extends FloatingContentProps {
  onEscapeKeyDown?: (event: KeyEvent) => void
}
export function SelectContent(props: SelectContentProps): JSX.Element {
  const state = context("SelectContent")
  return createComponent(Show, {
    get when() { return state.open() },
    keyed: true,
    get children() {
      return createComponent(DismissableLayer, {
        onEscapeKeyDown(event) {
          props.onEscapeKeyDown?.(event)
          state.setOpen(false)
        },
        get children() {
          return FloatingLayer({
            ...props,
            // Focused only to receive keys; the highlighted item shows where
            // the user is, so the popup gets no focus look of its own.
            get style() { return { keyboardFocusDim: false, ...props.style } },
            autoFocus: true,
            tabIndex: props.tabIndex ?? -1,
            onMouseDownOutside(event) {
              props.onMouseDownOutside?.(event)
              state.setOpen(false, false)
            },
            onKeyDown(event) {
              props.onKeyDown?.(event)
              // The popup is modal, like Base UI: Tab stays inside until it closes.
              if (event.key === "tab") event.preventDefault()
              else if (event.key === "down") state.move(1)
              else if (event.key === "up") state.move(-1)
              else if ((event.key === "enter" || event.key === "space") && state.active()) {
                state.select(state.active()!)
              }
            },
          })
        },
      })
    },
  })
}

export interface SelectItemState { selected: boolean; highlighted: boolean; disabled: boolean }
export interface SelectItemProps extends Omit<HostProps, "style"> {
  value: string
  disabled?: boolean
  asChild?: boolean
  children?: JSX.Element | ((state: SelectItemState) => JSX.Element)
  style?: StateStyle<SelectItemState>
}
export function SelectItem(props: SelectItemProps): JSX.Element {
  const state = context("SelectItem")
  onMount(() => onCleanup(state.register({ value: props.value, disabled: props.disabled ?? false })))
  const itemState = () => ({
    selected: state.value() === props.value,
    highlighted: state.active() === props.value,
    disabled: props.disabled ?? false,
  })
  return renderSlot({
    asChild: props.asChild,
    get children() {
      return typeof props.children === "function" ? props.children(itemState()) : props.children
    },
    props: {
      ...props,
      get style() { return resolveStyle(props.style, itemState()) },
      onMouseEnter(event: HostMouseEvent) {
        props.onMouseEnter?.(event)
        if (!itemState().disabled) state.setActive(props.value)
      },
      onClick(event: HostPointerEvent) {
        props.onClick?.(event)
        if (!itemState().disabled) state.select(props.value)
      },
    },
  })
}

function divComponent(props: HostProps & { children?: JSX.Element }): JSX.Element {
  return jsx("div", props as Record<string, unknown>)
}
export const SelectGroup = divComponent
export const SelectLabel = divComponent
export const SelectSeparator = divComponent
export const SelectScrollUpButton = divComponent
export const SelectScrollDownButton = divComponent

export {
  Select as Root,
  SelectContent as Content,
  SelectGroup as Group,
  SelectItem as Item,
  SelectLabel as Label,
  SelectScrollDownButton as ScrollDownButton,
  SelectScrollUpButton as ScrollUpButton,
  SelectSeparator as Separator,
  SelectTrigger as Trigger,
  SelectValue as Value,
}
