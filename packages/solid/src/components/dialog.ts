/** Headless Base UI-shaped Dialog over a full-window GPUIX anchored layer. */

import {
  createComponent,
  createContext,
  createSignal,
  Show,
  splitProps,
  useContext,
  type JSX,
} from "solid-js"
import { resolveFocusTarget } from "@ghia/native/host"
import type { FocusTarget, HostPointerEvent, HostProps, KeyEvent } from "@ghia/native/host"
import type { HostElement } from "../host.js"
import { jsx } from "../jsx-runtime.js"
import { useGpuixRequired } from "../root.js"
import { buttonProps } from "./button.js"
import { DismissableLayer, overrideProps, renderSlot } from "./floating.js"

interface DialogState {
  open(): boolean
  modal(): boolean
  setOpen(open: boolean): void
  pointerDismissal(): boolean
  trigger: { current: HostElement | null }
}

const DialogContext = createContext<DialogState>()
function context(name: string): DialogState {
  const value = useContext(DialogContext)
  if (!value) throw new Error(`${name} must be used inside Dialog`)
  return value
}

export interface DialogProps {
  children?: JSX.Element
  open?: boolean
  defaultOpen?: boolean
  onOpenChange?: (open: boolean) => void
  /** Default true: blocks the page behind, keeps Tab inside the popup. */
  modal?: boolean
  /** Keep the dialog open when the backdrop is pressed. */
  disablePointerDismissal?: boolean
}

export function Dialog(props: DialogProps): JSX.Element {
  const [internalOpen, setInternalOpen] = createSignal(props.defaultOpen ?? false)
  const open = () => props.open === undefined ? internalOpen() : props.open
  const state: DialogState = {
    open,
    modal: () => props.modal ?? true,
    pointerDismissal: () => !props.disablePointerDismissal,
    trigger: { current: null },
    setOpen(next) {
      const previous = open()
      if (previous === next) return
      if (props.open === undefined) setInternalOpen(next)
      props.onOpenChange?.(next)
    },
  }
  return createComponent(DialogContext.Provider, {
    value: state,
    get children() { return props.children },
  })
}

export interface DialogTriggerProps extends HostProps {
  children?: JSX.Element
  asChild?: boolean
  ref?: (element: HostElement) => void
}

export function DialogTrigger(props: DialogTriggerProps): JSX.Element {
  const state = context("DialogTrigger")
  const renderer = useGpuixRequired()
  return renderSlot({
    asChild: props.asChild,
    children: props.children,
    props: Object.defineProperties(
      overrideProps(props, {
        ref(element: HostElement) {
          state.trigger.current = element
          props.ref?.(element)
        },
        get "aria-expanded"() { return state.open() },
      }, ["children"]),
      Object.getOwnPropertyDescriptors(buttonProps({
        get tabIndex() { return props.tabIndex },
        onKeyDown: (event) => props.onKeyDown?.(event),
        onKeyUp: (event) => props.onKeyUp?.(event),
        onClick(event) {
          props.onClick?.(event)
          state.setOpen(true)
        },
      }, renderer))
    ),
    defaultTabIndex: 0,
  })
}

export interface DialogPortalProps extends HostProps {
  children?: JSX.Element
}

/**
 * A full-window layer painted after the page, so it covers `<virtual-list>`
 * and everything else. Mounted only while the dialog is open.
 */
export function DialogPortal(props: DialogPortalProps): JSX.Element {
  const state = context("DialogPortal")
  return createComponent(Show, {
    get when() { return state.open() },
    keyed: true,
    get children() {
      return jsx("anchored", overrideProps(props, {
        // Native sizes the layer to the viewport, so a resize never leaves an
        // uncovered strip that clicks could reach.
        fill: "window",
        deferred: true,
        // Below menus and tooltips (priority 1), so a Select inside the popup
        // opens on top of it.
        priority: 0,
        get style() {
          return {
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            // Explicit transparent: the Backdrop paints the dim, not the layer.
            backgroundColor: "transparent",
            ...props.style,
            pointerEvents: state.modal() ? props.style?.pointerEvents : "none",
          }
        },
        get children() { return props.children },
      }))
    },
  })
}

export interface DialogBackdropProps extends HostProps {
  children?: JSX.Element
}

/**
 * Covers the window behind the popup. A click on it closes the dialog: a click
 * only reaches the backdrop when the press and the release were both on it.
 */
export function DialogBackdrop(props: DialogBackdropProps): JSX.Element {
  const state = context("DialogBackdrop")
  return jsx("div", overrideProps(props, {
    get style() {
      return { position: "absolute", top: 0, right: 0, bottom: 0, left: 0, ...props.style }
    },
    onClick(event: HostPointerEvent) {
      props.onClick?.(event)
      if (state.pointerDismissal()) state.setOpen(false)
    },
  }))
}

export interface DialogPopupProps extends HostProps {
  children?: JSX.Element
  ref?: (element: HostElement) => void
  /** Focus on open. Default: the popup itself, so the first Tab enters it. */
  initialFocus?: FocusTarget<HostElement>
  /** Focus on close. Default: the trigger, else the element focused before open. */
  finalFocus?: FocusTarget<HostElement>
}

/** The dialog surface. Moves focus in on open and back out on close. */
export function DialogPopup(allProps: DialogPopupProps): JSX.Element {
  const [focus, props] = splitProps(allProps, ["initialFocus", "finalFocus"])
  const state = context("DialogPopup")
  const renderer = useGpuixRequired()
  let popup: HostElement | null = null
  return createComponent(Show, {
    get when() { return state.open() },
    keyed: true,
    get children() {
      return createComponent(DismissableLayer, {
        onEscapeKeyDown: () => state.setOpen(false),
        initialFocus: () => resolveFocusTarget(focus.initialFocus, () => popup?.id ?? null),
        finalFocus: (previous) =>
          resolveFocusTarget(focus.finalFocus, () => state.trigger.current?.id ?? previous),
        get children() {
          return jsx("div", overrideProps(props, {
            ref(element: HostElement) {
              popup = element
              props.ref?.(element)
            },
            get role() { return props.role ?? "dialog" },
            get tabIndex() { return props.tabIndex ?? -1 },
            // A press inside the popup must never reach the backdrop behind it:
            // the backdrop is positioned, so the popup is too, and being later in
            // the tree it paints above it.
            // The popup is focused so the first Tab enters it; it is a surface,
            // not a control, so it gets no focus look of its own.
            get style() {
              return { position: "relative", pointerEvents: "auto", keyboardFocusDim: false, ...props.style }
            },
            onKeyDown(event: KeyEvent) {
              props.onKeyDown?.(event)
              if (!state.modal() || event.key !== "Tab" || event.defaultPrevented || !popup) return
              event.preventDefault()
              if (event.modifiers?.shift) renderer.focusPreviousWithin?.(popup.id)
              else renderer.focusNextWithin?.(popup.id)
            },
          }))
        },
      })
    },
  })
}

export function DialogTitle(props: HostProps & { children?: JSX.Element }): JSX.Element {
  return jsx("div", Object.defineProperties(
    { role: "heading", "aria-level": 2 },
    Object.getOwnPropertyDescriptors(props)
  ))
}

export function DialogDescription(props: HostProps & { children?: JSX.Element }): JSX.Element {
  return jsx("div", props as Record<string, unknown>)
}

export interface DialogCloseProps extends HostProps {
  children?: JSX.Element
  asChild?: boolean
}

export function DialogClose(props: DialogCloseProps): JSX.Element {
  const state = context("DialogClose")
  const renderer = useGpuixRequired()
  return renderSlot({
    asChild: props.asChild,
    children: props.children,
    props: Object.defineProperties(
      overrideProps(props, {}, ["children"]),
      Object.getOwnPropertyDescriptors(buttonProps({
        get tabIndex() { return props.tabIndex },
        onKeyDown: (event) => props.onKeyDown?.(event),
        onKeyUp: (event) => props.onKeyUp?.(event),
        onClick(event) {
          props.onClick?.(event)
          state.setOpen(false)
        },
      }, renderer))
    ),
    defaultTabIndex: 0,
  })
}

export {
  Dialog as Root,
  DialogBackdrop as Backdrop,
  DialogClose as Close,
  DialogDescription as Description,
  DialogPopup as Popup,
  DialogPortal as Portal,
  DialogTitle as Title,
  DialogTrigger as Trigger,
}
