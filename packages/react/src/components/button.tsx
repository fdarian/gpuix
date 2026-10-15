/** Headless Base UI-shaped Button: a focusable `div` that acts like `<button>`. */

import { forwardRef } from "react"
import type { ReactElement, ReactNode } from "react"
import type { HostPointerEvent, KeyEvent, Props, PublicInstance, StyleDesc } from "../types/host.js"
import { renderSlot, resolveStyle, type StateStyle } from "./floating.js"

export interface ButtonState {
  disabled: boolean
}

export interface ButtonBehavior {
  disabled?: boolean
  /** Keep a disabled button in the Tab order, for example while it saves. */
  focusableWhenDisabled?: boolean
  onClick?: (event: HostPointerEvent) => void
  onKeyDown?: (event: KeyEvent) => void
  onKeyUp?: (event: KeyEvent) => void
  tabIndex?: number
}

/**
 * Props that make any element act like a `<button>`: a tab stop, `role`, and
 * `onClick` from a press, Enter (on key down) or Space (on key up), like a
 * browser: a key `click()`s the element, so the handler gets a DOM click that
 * bubbles. Primitives such as `Dialog.Trigger` build on it.
 */
export function buttonProps({
  disabled = false,
  focusableWhenDisabled = false,
  onClick,
  onKeyDown,
  onKeyUp,
  tabIndex,
}: ButtonBehavior): Props {
  return {
    role: "button",
    // Unset keeps an `asChild` child's own tabIndex; renderSlot defaults to 0.
    tabIndex: disabled && !focusableWhenDisabled ? -1 : tabIndex,
    onClick: (event: HostPointerEvent) => {
      if (!disabled) onClick?.(event)
    },
    onKeyDown: (event: KeyEvent) => {
      onKeyDown?.(event)
      if (disabled || event.defaultPrevented) return
      if (event.key === "enter" && !event.isHeld) event.click()
      // Space would otherwise scroll or type; it fires on release.
      if (event.key === "space") event.preventDefault()
    },
    onKeyUp: (event: KeyEvent) => {
      onKeyUp?.(event)
      if (!disabled && !event.defaultPrevented && event.key === "space") event.click()
    },
  }
}

export interface ButtonProps extends Omit<Props, "style" | "onClick" | "onKeyDown" | "onKeyUp">, ButtonBehavior {
  children?: ReactNode
  asChild?: boolean
  style?: StateStyle<ButtonState>
}

export const Button = forwardRef<PublicInstance, ButtonProps>(function Button(
  {
    asChild,
    children,
    style,
    disabled = false,
    focusableWhenDisabled,
    onClick,
    onKeyDown,
    onKeyUp,
    tabIndex,
    ...props
  },
  ref
): ReactElement {
  const state: ButtonState = { disabled }
  return renderSlot({
    asChild,
    children,
    props: {
      ...props,
      ...buttonProps({ disabled, focusableWhenDisabled, onClick, onKeyDown, onKeyUp, tabIndex }),
      style: resolveStyle(style, state) as StyleDesc | undefined,
    },
    ref,
    defaultTabIndex: 0,
  })
})
