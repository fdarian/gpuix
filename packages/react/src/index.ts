// GPUIX React - React bindings for GPUI
import { document as sharedDocument } from "@gpuix/native/host"
import type { GpuixDocument } from "@gpuix/native/host"
import type { PublicInstance } from "./types/host.js"
export { createRoot, flushSync } from "./reconciler/index.js"
export { createPortal } from "./reconciler/portal.js"
export {
  createRenderer,
  enableAutomation,
  render,
  resetRender,
  startFrameLoop,
} from "./reconciler/renderer.js"
export { GpuixContext, useGpuix, useGpuixRequired } from "./hooks/use-gpuix.js"
export { useWindowInsets, useWindowSize } from "./hooks/use-window-size.js"
export { useMatchMedia, useMediaQuery, useReducedMotion } from "./hooks/use-media-query.js"
export {
  useMotionValue,
  useMotionValueEvent,
  useSpring,
  useTransform,
  useVelocity,
} from "./hooks/use-motion-value.js"
export { findRanges, useTextSearch } from "./hooks/use-text-search.js"
export {
  Animation,
  animate,
  CSSAnimation,
  CSSTransition,
  defineKeyframes,
  isMotionValue,
  motionValue,
} from "@gpuix/native/host"
export type {
  AnimationPlaybackControls,
  AnimationPlaybackControlsWithThen,
  AnimationPlaybackState,
  AnimationPlayState,
  KeyframesDefinition,
  KeyframeStyle,
  MotionSpringConfig,
  MotionTransformOptions,
  MotionValue,
  MotionValueEvents,
} from "@gpuix/native/host"
export {
  clipboard,
  MediaQueryList,
  MediaQueryListEvent,
  createTextSearchController,
  IntersectionObserver,
  matchMedia,
  observeAppearance,
  observeSelectedText,
  observeWindowInsets,
  observeWindowSize,
  pushDismissLayer,
  readWindowInsets,
  readWindowSize,
  window,
  ResizeObserver,
} from "@gpuix/native/host"
/** `activeElement` and `pointerLockElement` are elements as a `ref` gives them. */
export const document = sharedDocument as GpuixDocument<PublicInstance>
export type {
  Appearance,
  AddEventListenerOptions,
  BoundingClientRect,
  Clipboard,
  DismissLayer,
  DomEvent,
  RootEvent,
  WindowEventListener,
  IntersectionObserverCallback,
  IntersectionObserverEntry,
  IntersectionObserverInit,
  MatchMedia,
  ObservableElement,
  ResizeObserverBoxOptions,
  ResizeObserverCallback,
  ResizeObserverEntry,
  ResizeObserverOptions,
  ResizeObserverSize,
} from "@gpuix/native/host"
export type {
  FindRangesOptions,
  TextSearch,
  TextSearchOptions,
} from "./hooks/use-text-search.js"
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
export type {
  ComboboxInputProps,
  ComboboxItemProps,
  ComboboxItemState,
  ComboboxListProps,
  ComboboxProps,
  ComboboxTriggerProps,
  ComboboxValueProps,
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
export { DismissableLayer } from "./components/floating.js"
export type { DismissableLayerProps } from "./components/floating.js"
export type {
  DialogBackdropProps,
  DialogCloseProps,
  DialogPopupProps,
  DialogPortalProps,
  DialogProps,
  DialogTriggerProps,
} from "./components/dialog.js"
export { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from "./components/tooltip.js"
export type {
  TooltipContentProps,
  TooltipProps,
  TooltipProviderProps,
  TooltipTriggerProps,
} from "./components/tooltip.js"
export {
  AnimatePresence,
  LayoutGroup,
  MotionConfig,
  UniversalStyle,
  motion,
  useIsPresent,
  usePresence,
  useReducedMotionConfig,
} from "./components/index.js"
export type {
  AnimatePresenceProps,
  LayoutGroupProps,
  MotionConfigProps,
  UniversalStyleProps,
} from "./components/index.js"
export { Reorder } from "./components/reorder.js"
export type { ReorderGroupProps, ReorderItemProps } from "./components/reorder.js"
export type { Root, FrameLoop, RenderOptions } from "./reconciler/renderer.js"
export type {
  WindowInsets,
  WindowInsetsOptions,
  WindowSize,
  WindowSizeOptions,
} from "./hooks/use-window-size.js"

// Re-export types
export type { MotionDivProps, MotionShapeProps, MotionSpanProps } from "./components/index.js"
export type {
  AnchoredProps,
  CodeProps,
  CursorValue,
  DebugFrameOverlayMode,
  DebugFrameOverlayStats,
  DiffProps,
  DimensionValue,
  EdgeInsets,
  ElementBounds,
  GpuixMetrics,
  GpuixTheme,
  HighlightMatch,
  HighlightSpec,
  HostDragEvent,
  HostMouseEvent,
  HostPointerEvent,
  ImgInstance,
  ImgProps,
  NativeviewProps,
  WebviewInstance,
  WebviewProps,
  FocusTarget,
  InputProps,
  KeyEvent,
  LinearGradientBackground,
  LinearGradientStop,
  MarkdownProps,
  MotionCurve,
  MotionEase,
  MotionProps,
  MotionSpanStyle,
  MotionSpanVariant,
  MotionSpanVariants,
  MotionSpringTransition,
  MotionStyle,
  MotionTransition,
  MotionTweenTransition,
  MotionValueStyle,
  MotionVariantLabels,
  MutationHost,
  NativeRenderer,
  NativeWindowInsets,
  PathPromptOptions,
  Props,
  PublicInstance,
  StyleDesc,
  SvgCircleProps,
  SvgClipPathProps,
  SvgDefsProps,
  SvgEllipseProps,
  SvgGradientProps,
  SvgGroupProps,
  SvgLineProps,
  SvgMaskProps,
  SvgMotionProps,
  SvgMotionStyle,
  SvgMotionTransition,
  SvgPathProps,
  SvgPolyProps,
  SvgProps,
  SvgRectProps,
  SvgStopProps,
  SvgSymbolProps,
  SvgUseProps,
  SvgTextProps,
  SvgPatternProps,
  SvgMarkerProps,
  SvgImageProps,
  SvgTspanProps,
  SvgTextPathProps,
  SvgFilterProps,
  SvgFilterPrimitiveProps,
  SvgFeBlendProps,
  SvgFeColorMatrixProps,
  SvgFeComponentTransferProps,
  SvgFeFuncProps,
  SvgFeCompositeProps,
  SvgFeConvolveMatrixProps,
  SvgFeDiffuseLightingProps,
  SvgFeDisplacementMapProps,
  SvgFeDistantLightProps,
  SvgFeDropShadowProps,
  SvgFeFloodProps,
  SvgFeGaussianBlurProps,
  SvgFeImageProps,
  SvgFeMergeProps,
  SvgFeMergeNodeProps,
  SvgFeMorphologyProps,
  SvgFeOffsetProps,
  SvgFePointLightProps,
  SvgFeSpecularLightingProps,
  SvgFeSpotLightProps,
  SvgFeTileProps,
  SvgFeTurbulenceProps,
  SelectionHost,
  SyntaxTheme,
  TextareaProps,
  VirtualListProps,
  WindowInsetsHost,
  WindowKeyEventHandler,
  WindowKeyEventHandlers,
  WindowSizeHost,
} from "./types/host.js"
export { handleGpuixEvent } from "./reconciler/event-registry.js"
export {
  applyMacCpuThrottleFromEnv,
  MAC_CPU_THROTTLES,
  readMacCpuThrottle,
} from "./cpu-throttle.js"
export type { MacCpuThrottle } from "./cpu-throttle.js"
export { screen } from "@gpuix/native/screen"
export type { Display, Rectangle, Size } from "@gpuix/native/screen"
export { collectGarbageWhenIdle } from "./idle-gc.js"
export type { IdleGcOptions } from "./idle-gc.js"
export type {
  EventPayload,
  EventModifiers,
  WindowOptions,
  WindowSize as NativeWindowSize,
} from "@gpuix/native"

export { GpuixRenderer } from "@gpuix/native"
