/** Headless Base UI-shaped Dialog over a full-window GPUIX anchored layer. */

import React, {
  createContext,
  forwardRef,
  useCallback,
  useContext,
  useRef,
} from "react"
import type { ReactElement, ReactNode } from "react"
import { resolveFocusTarget } from "@gpuix/native/host"
import type { FocusTarget, HostPointerEvent, KeyEvent, Props, PublicInstance } from "../types/host.js"
import { useGpuix, useGpuixRequired } from "../hooks/use-gpuix.js"
import { buttonProps } from "./button.js"
import {
  renderSlot,
  setRefs,
  useControllableState,
  DismissableLayer,
} from "./floating.js"

interface DialogContextValue {
  open: boolean
  modal: boolean
  disablePointerDismissal: boolean
  setOpen: (open: boolean) => void
  triggerRef: React.MutableRefObject<PublicInstance | null>
}

const DialogContext = createContext<DialogContextValue | null>(null)

function useDialogContext(name: string): DialogContextValue {
  const context = useContext(DialogContext)
  if (!context) throw new Error(`${name} must be used inside Dialog`)
  return context
}

export interface DialogProps {
  children?: ReactNode
  open?: boolean
  defaultOpen?: boolean
  onOpenChange?: (open: boolean) => void
  /** Default true: blocks the page behind, keeps Tab inside the popup. */
  modal?: boolean
  /** Keep the dialog open when the backdrop is pressed. */
  disablePointerDismissal?: boolean
}

export function Dialog({
  children,
  open: openProp,
  defaultOpen = false,
  onOpenChange,
  modal = true,
  disablePointerDismissal = false,
}: DialogProps): ReactElement {
  const [open, setOpen] = useControllableState({
    value: openProp,
    defaultValue: defaultOpen,
    onChange: onOpenChange,
  })
  const triggerRef = useRef<PublicInstance | null>(null)
  const context: DialogContextValue = {
    open,
    modal,
    disablePointerDismissal,
    setOpen,
    triggerRef,
  }
  return <DialogContext.Provider value={context}>{children}</DialogContext.Provider>
}

export interface DialogTriggerProps extends Props {
  asChild?: boolean
}

export const DialogTrigger = forwardRef<PublicInstance, DialogTriggerProps>(
  function DialogTrigger({ asChild, children, onClick, onKeyDown, onKeyUp, ...props }, forwardedRef) {
    const context = useDialogContext("DialogTrigger")
    const renderer = useGpuixRequired()
    const { triggerRef } = context
    // Stable, so React does not detach it (set null) in the commit that
    // unmounts the popup, which is exactly when the popup reads it.
    const ref = useCallback((value: PublicInstance | null) => {
      triggerRef.current = value
      setRefs(value, forwardedRef)
    }, [triggerRef, forwardedRef])
    return renderSlot({
      asChild,
      children,
      props: {
        ...props,
        "aria-expanded": context.open,
        ...buttonProps({
          tabIndex: props.tabIndex,
          onKeyDown,
          onKeyUp,
          onClick: (event) => {
            onClick?.(event)
            context.setOpen(true)
          },
        }, renderer),
      },
      ref,
      defaultTabIndex: 0,
    })
  }
)

export interface DialogPortalProps extends Props {}

/**
 * A full-window layer painted after the page, so it covers `<virtual-list>`
 * and everything else. Mounted only while the dialog is open.
 */
export const DialogPortal = forwardRef<PublicInstance, DialogPortalProps>(
  function DialogPortal({ style, children, ...props }, ref) {
    const context = useDialogContext("DialogPortal")
    if (!context.open) return null
    const { modal } = context
    return (
      <anchored
        {...props}
        ref={ref}
        // Native sizes the layer to the viewport, so a resize never leaves an
        // uncovered strip that clicks could reach.
        fill="window"
        deferred
        // Below menus and tooltips (priority 1), so a Select inside the popup
        // opens on top of it.
        priority={0}
        style={{
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          // Explicit transparent: the Backdrop paints the dim, not the layer.
          backgroundColor: "transparent",
          ...style,
          pointerEvents: modal ? style?.pointerEvents : "none",
        }}
      >
        {children}
      </anchored>
    )
  }
)

export interface DialogBackdropProps extends Props {}

/**
 * Covers the window behind the popup. A click on it closes the dialog: a click
 * only reaches the backdrop when the press and the release were both on it.
 */
export const DialogBackdrop = forwardRef<PublicInstance, DialogBackdropProps>(
  function DialogBackdrop({ style, onClick, ...props }, ref) {
    const context = useDialogContext("DialogBackdrop")
    return (
      <div
        {...props}
        ref={ref}
        style={{ position: "absolute", top: 0, right: 0, bottom: 0, left: 0, ...style }}
        onClick={(event: HostPointerEvent) => {
          onClick?.(event)
          if (!context.disablePointerDismissal) context.setOpen(false)
        }}
      />
    )
  }
)

export interface DialogPopupProps extends Props {
  /** Focus on open. Default: the popup itself, so the first Tab enters it. */
  initialFocus?: FocusTarget<PublicInstance>
  /** Focus on close. Default: the trigger, else the element focused before open. */
  finalFocus?: FocusTarget<PublicInstance>
}

/** The dialog surface. Moves focus in on open and back out on close. */
export const DialogPopup = forwardRef<PublicInstance, DialogPopupProps>(
  function DialogPopup(
    { style, onKeyDown, initialFocus, finalFocus, tabIndex = -1, role = "dialog", ...props },
    forwardedRef
  ) {
    const context = useDialogContext("DialogPopup")
    const { renderer } = useGpuix()
    const popupRef = useRef<PublicInstance | null>(null)
    if (!context.open) return null
    return (
      <DismissableLayer
        onEscapeKeyDown={() => context.setOpen(false)}
        initialFocus={() => resolveFocusTarget(initialFocus, () => popupRef.current?.id ?? null)}
        finalFocus={(previous) =>
          resolveFocusTarget(finalFocus, () => context.triggerRef.current?.id ?? previous)
        }
      >
        <div
          {...props}
          ref={(value: PublicInstance | null) => {
            popupRef.current = value
            setRefs(value, forwardedRef)
          }}
          role={role}
          tabIndex={tabIndex}
          // A press inside the popup must never reach the backdrop behind it:
          // the backdrop is positioned, so the popup is too, and being later in
          // the tree it paints above it.
          // The popup is focused so the first Tab enters it; it is a surface,
          // not a control, so it gets no focus look of its own.
          style={{ position: "relative", pointerEvents: "auto", keyboardFocusDim: false, ...style }}
          onKeyDown={(event: KeyEvent) => {
            onKeyDown?.(event)
            if (!context.modal || event.key !== "Tab" || event.defaultPrevented) return
            const popup = popupRef.current
            if (!popup) return
            event.preventDefault()
            if (event.modifiers?.shift) renderer?.focusPreviousWithin?.(popup.id)
            else renderer?.focusNextWithin?.(popup.id)
          }}
        />
      </DismissableLayer>
    )
  }
)

export const DialogTitle = forwardRef<PublicInstance, Props>(function DialogTitle(props, ref) {
  return <div role="heading" aria-level={2} {...props} ref={ref} />
})

export const DialogDescription = forwardRef<PublicInstance, Props>(
  function DialogDescription(props, ref) {
    return <div {...props} ref={ref} />
  }
)

export interface DialogCloseProps extends Props {
  asChild?: boolean
}

export const DialogClose = forwardRef<PublicInstance, DialogCloseProps>(
  function DialogClose({ asChild, children, onClick, onKeyDown, onKeyUp, ...props }, ref) {
    const context = useDialogContext("DialogClose")
    const renderer = useGpuixRequired()
    return renderSlot({
      asChild,
      children,
      props: {
        ...props,
        ...buttonProps({
          tabIndex: props.tabIndex,
          onKeyDown,
          onKeyUp,
          onClick: (event) => {
            onClick?.(event)
            context.setOpen(false)
          },
        }, renderer),
      },
      ref,
      defaultTabIndex: 0,
    })
  }
)

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
