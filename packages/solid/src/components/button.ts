/** Headless Base UI-shaped Button: a focusable `div` that acts like `<button>`. */

import { splitProps, type JSX } from "solid-js"
import type { HostPointerEvent, HostProps, KeyEvent } from "@gpuix/native/host"
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
 * Enter and Space activate a button through `onClick` with the key event, which
 * has no `target` or `path`. It is not a DOM click, and handlers that read
 * those fields see them missing on keyboard activation.
 */
function asClickEvent(event: KeyEvent): HostPointerEvent {
  return event as unknown as HostPointerEvent
}

/**
 * Props that make any element act like a `<button>`: a tab stop, `role`, and
 * `onClick` from a press, Enter (on key down) or Space (on key up), like a
 * browser. Reads `behavior` lazily, so a reactive `disabled` stays live.
 * Primitives such as `DialogTrigger` build on it.
 */
export function buttonProps(behavior: ButtonBehavior): Record<string, unknown> {
  const disabled = () => behavior.disabled ?? false
  return {
    role: "button",
    // Unset keeps an `asChild` child's own tabIndex; renderSlot defaults to 0.
    get tabIndex() {
      return disabled() && !behavior.focusableWhenDisabled ? -1 : behavior.tabIndex
    },
    onClick(event: HostPointerEvent) {
      if (!disabled()) behavior.onClick?.(event)
    },
    onKeyDown(event: KeyEvent) {
      behavior.onKeyDown?.(event)
      if (disabled() || event.defaultPrevented) return
      if (event.key === "enter" && !event.isHeld) behavior.onClick?.(asClickEvent(event))
      // Space would otherwise scroll or type; it fires on release.
      if (event.key === "space") event.preventDefault()
    },
    onKeyUp(event: KeyEvent) {
      behavior.onKeyUp?.(event)
      if (!disabled() && !event.defaultPrevented && event.key === "space") {
        behavior.onClick?.(asClickEvent(event))
      }
    },
  }
}

export interface ButtonProps
  extends Omit<HostProps, "style" | "onClick" | "onKeyDown" | "onKeyUp">, ButtonBehavior {
  children?: JSX.Element
  asChild?: boolean
  style?: StateStyle<ButtonState>
}

export function Button(allProps: ButtonProps): JSX.Element {
  const [behavior, local, props] = splitProps(
    allProps,
    ["disabled", "focusableWhenDisabled", "onClick", "onKeyDown", "onKeyUp", "tabIndex"],
    ["asChild", "children", "style"]
  )
  const button = buttonProps(behavior)
  return renderSlot({
    asChild: local.asChild,
    children: local.children,
    props: Object.defineProperties(
      { ...props },
      {
        ...Object.getOwnPropertyDescriptors(button),
        style: {
          enumerable: true,
          get: () => resolveStyle(local.style, { disabled: behavior.disabled ?? false }),
        },
      }
    ),
    defaultTabIndex: 0,
  })
}
