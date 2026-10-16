/** Headless Base UI-shaped Button: a focusable `div` that acts like `<button>`. */

import { splitProps, type JSX } from "solid-js"
import { createElementMethods } from "@gpuix/native/host"
import type { HostDomEvent, HostPointerEvent, HostProps, KeyEvent, NativeRenderer } from "@gpuix/native/host"
import { useGpuixRequired } from "../root.js"
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
 * bubbles. Reads `behavior` lazily, so a reactive `disabled` stays live.
 * Primitives such as `DialogTrigger` build on it. `renderer` delivers that click.
 */
export function buttonProps(
  behavior: ButtonBehavior,
  renderer: NativeRenderer
): Record<string, unknown> {
  // Only the element the key was aimed at acts: a key bubbling from a nested
  // buttonProps element must not click this one too, like Base UI's useButton.
  const isTarget = (event: KeyEvent) => {
    const keyEvent = event as KeyEvent & Pick<HostDomEvent, "target" | "currentTarget">
    return keyEvent.target === keyEvent.currentTarget
  }
  const click = (event: KeyEvent) => {
    event.preventDefault()
    createElementMethods(renderer, event.elementId).click()
  }
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
      if (disabled() || event.defaultPrevented || !isTarget(event)) return
      if (event.key === "Enter" && !event.repeat) click(event)
      // Space would otherwise scroll or type; it fires on release.
      if (event.key === " ") event.preventDefault()
    },
    onKeyUp(event: KeyEvent) {
      behavior.onKeyUp?.(event)
      if (!disabled() && !event.defaultPrevented && isTarget(event) && event.key === " ") {
        click(event)
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
  const button = buttonProps(behavior, useGpuixRequired())
  return renderSlot({
    asChild: local.asChild,
    children: local.children,
    props: Object.defineProperties(
      Object.defineProperties({}, Object.getOwnPropertyDescriptors(props)),
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
