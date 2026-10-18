export {
  createComponent,
  createElement,
  createTextNode,
  effect,
  insert,
  insertNode,
  memo,
  mergeProps,
  setProp,
  spread,
  use,
} from "./universal.js"
export { createRoot, GpuixContext, useGpuix, useGpuixRequired } from "./root.js"
export type { HostElement } from "./host.js"
import { document as sharedDocument, window } from "@gpuix/native/host"
import type { GpuixDocument } from "@gpuix/native/host"
import type { HostElement } from "./host.js"
export { window }
/** `activeElement` and `pointerLockElement` are `HostElement`s, the objects a `ref` gives. */
export const document = sharedDocument as GpuixDocument<HostElement>
export type {
  AddEventListenerOptions,
  DomEvent,
  RootEvent,
  WindowEventListener,
} from "@gpuix/native/host"
export { createRenderer, render, resetRender } from "./renderer.js"
export { MediaQueryList, MediaQueryListEvent, matchMedia } from "@gpuix/native/host"
export type { MatchMedia } from "@gpuix/native/host"
export {
  createAppearance,
  createMatchMedia,
  createMediaQuery,
  createReducedMotion,
  createSelectedText,
  createTextSearch,
  createWindowInsets,
  createWindowSize,
} from "./primitives.js"
export {
  createMotionValue,
  createMotionValueEvent,
  createSpring,
  createTransform,
  createVelocity,
} from "./motion-value.js"
export { motion } from "./components/motion.js"
export { LayoutGroup } from "./components/layout-group.js"
export type { LayoutGroupProps } from "./components/layout-group.js"
export { Reorder } from "./components/reorder.js"
export type { ReorderGroupProps, ReorderItemProps } from "./components/reorder.js"
export { MotionConfig, createReducedMotionConfig } from "./components/motion-config.js"
export type { MotionConfigProps } from "./components/motion-config.js"
export { UniversalStyle } from "./components/universal-style.js"
export type { UniversalStyleProps } from "./components/universal-style.js"
export {
  AnimatePresence,
  useIsPresent,
  usePresence,
} from "./components/animate-presence.js"
export type { AnimatePresenceProps } from "./components/animate-presence.js"
export {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectLabel,
  SelectScrollDownButton,
  SelectScrollUpButton,
  SelectSeparator,
  SelectTrigger,
  SelectValue,
} from "./components/select.js"
export {
  Combobox,
  ComboboxContent,
  ComboboxEmpty,
  ComboboxGroup,
  ComboboxInput,
  ComboboxItem,
  ComboboxLabel,
  ComboboxList,
  ComboboxSeparator,
  ComboboxTrigger,
  ComboboxValue,
} from "./components/combobox.js"
export { Button, buttonProps } from "./components/button.js"
export type { ButtonBehavior, ButtonProps, ButtonState } from "./components/button.js"
export {
  Dialog,
  DialogBackdrop,
  DialogClose,
  DialogDescription,
  DialogPopup,
  DialogPortal,
  DialogTitle,
  DialogTrigger,
} from "./components/dialog.js"
export type {
  DialogBackdropProps,
  DialogCloseProps,
  DialogPopupProps,
  DialogPortalProps,
  DialogProps,
  DialogTriggerProps,
} from "./components/dialog.js"
export {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "./components/tooltip.js"
export {
  FloatingLayer,
  floatingRootStyle,
  mergeStyles,
  renderSlot,
  resolveStyle,
  DismissableLayer,
} from "./components/floating.js"
export * from "@gpuix/native/host"
export { GpuixRenderer } from "@gpuix/native"
export type { EventModifiers, EventPayload, WindowOptions } from "@gpuix/native"
export type { JSX, SolidHostProps } from "./jsx-runtime.js"
export type { MotionDivProps, MotionShapeProps, MotionSpanProps } from "./components/motion.js"
export type {
  SelectContentProps,
  SelectItemData,
  SelectItemProps,
  SelectItemState,
  SelectProps,
  SelectTriggerProps,
  SelectTriggerState,
  SelectValueProps,
} from "./components/select.js"
export type {
  ComboboxInputProps,
  ComboboxItemProps,
  ComboboxItemState,
  ComboboxListProps,
  ComboboxProps,
  ComboboxTriggerProps,
  ComboboxValueProps,
} from "./components/combobox.js"
export type {
  TooltipContentProps,
  TooltipProps,
  TooltipProviderProps,
  TooltipTriggerProps,
} from "./components/tooltip.js"
