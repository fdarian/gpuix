import type { Key, ReactNode, Ref } from "react"
import type { EventPayload } from "@gpuix/native"
import { HOST_BINDING } from "@gpuix/native/host"
import type {
  BoundingClientRect,
  ElementIdAllocator,
  ElementType,
  EventHandlerMap,
  FocusableElement,
  FormMethods,
  FormProps as NativeFormProps,
  HostBinding,
  HostProps,
  ImagePixelsOptions,
  InputProps as NativeInputProps,
  LabelProps as NativeLabelProps,
  TextareaProps as NativeTextareaProps,
  ImgProps as NativeImgProps,
  SvgProps as NativeSvgProps,
  SvgCircleProps as NativeSvgCircleProps,
  SvgClipPathProps as NativeSvgClipPathProps,
  SvgDefsProps as NativeSvgDefsProps,
  SvgEllipseProps as NativeSvgEllipseProps,
  SvgGradientProps as NativeSvgGradientProps,
  SvgGroupProps as NativeSvgGroupProps,
  SvgLineProps as NativeSvgLineProps,
  SvgPathProps as NativeSvgPathProps,
  SvgPolyProps as NativeSvgPolyProps,
  SvgMaskProps as NativeSvgMaskProps,
  SvgRectProps as NativeSvgRectProps,
  SvgStopProps as NativeSvgStopProps,
  SvgSymbolProps as NativeSvgSymbolProps,
  SvgUseProps as NativeSvgUseProps,
  SvgTextProps as NativeSvgTextProps,
  SvgPatternProps as NativeSvgPatternProps,
  SvgMarkerProps as NativeSvgMarkerProps,
  SvgImageProps as NativeSvgImageProps,
  SvgTspanProps as NativeSvgTspanProps,
  SvgTextPathProps as NativeSvgTextPathProps,
  SvgFilterProps as NativeSvgFilterProps,
  SvgFilterPrimitiveProps as NativeSvgFilterPrimitiveProps,
  SvgFeBlendProps as NativeSvgFeBlendProps,
  SvgFeColorMatrixProps as NativeSvgFeColorMatrixProps,
  SvgFeComponentTransferProps as NativeSvgFeComponentTransferProps,
  SvgFeFuncProps as NativeSvgFeFuncProps,
  SvgFeCompositeProps as NativeSvgFeCompositeProps,
  SvgFeConvolveMatrixProps as NativeSvgFeConvolveMatrixProps,
  SvgFeDiffuseLightingProps as NativeSvgFeDiffuseLightingProps,
  SvgFeDisplacementMapProps as NativeSvgFeDisplacementMapProps,
  SvgFeDistantLightProps as NativeSvgFeDistantLightProps,
  SvgFeDropShadowProps as NativeSvgFeDropShadowProps,
  SvgFeFloodProps as NativeSvgFeFloodProps,
  SvgFeGaussianBlurProps as NativeSvgFeGaussianBlurProps,
  SvgFeImageProps as NativeSvgFeImageProps,
  SvgFeMergeProps as NativeSvgFeMergeProps,
  SvgFeMergeNodeProps as NativeSvgFeMergeNodeProps,
  SvgFeMorphologyProps as NativeSvgFeMorphologyProps,
  SvgFeOffsetProps as NativeSvgFeOffsetProps,
  SvgFePointLightProps as NativeSvgFePointLightProps,
  SvgFeSpecularLightingProps as NativeSvgFeSpecularLightingProps,
  SvgFeSpotLightProps as NativeSvgFeSpotLightProps,
  SvgFeTileProps as NativeSvgFeTileProps,
  SvgFeTurbulenceProps as NativeSvgFeTurbulenceProps,
  CodeProps as NativeCodeProps,
  DiffProps as NativeDiffProps,
  MarkdownProps as NativeMarkdownProps,
  AnchoredProps as NativeAnchoredProps,
  MotionBinding,
  MotionValueStyle,
  MutationRenderer,
  NativeRenderer,
  RootEventHandlers,
  TextControlSelection,
  ScrollableElement,
  WindowKeyEventHandlers,
} from "@gpuix/native/host"

export * from "@gpuix/native/host"

/** `onFocusIn` and `onFocusOut` do not exist in React, where `onFocus` and `onBlur` bubble. */
type ReactHostProps<Props> = Omit<Props, "onFocusIn" | "onFocusOut">

export interface Props extends Omit<ReactHostProps<HostProps>, "style"> {
  style?: MotionValueStyle
  key?: Key | null
  children?: ReactNode
  ref?: Ref<PublicInstance>
}

export interface InputProps extends ReactHostProps<NativeInputProps> {
  key?: Key | null
  children?: ReactNode
  ref?: Ref<TextControlInstance>
}

export interface LabelProps extends ReactHostProps<NativeLabelProps> {
  key?: Key | null
  children?: ReactNode
  ref?: Ref<PublicInstance>
}

export interface FormProps extends ReactHostProps<NativeFormProps> {
  key?: Key | null
  children?: ReactNode
  ref?: Ref<FormInstance>
}

export interface TextareaProps extends ReactHostProps<NativeTextareaProps> {
  key?: Key | null
  children?: ReactNode
  ref?: Ref<TextControlInstance>
}

export interface ImgProps extends ReactHostProps<NativeImgProps> {
  key?: Key | null
  children?: ReactNode
  ref?: Ref<ImgInstance>
}

export interface SvgProps extends ReactHostProps<NativeSvgProps> {
  key?: Key | null
  children?: ReactNode
  ref?: Ref<PublicInstance>
}

/** A shape inside an `<svg>`. React adds `key`, `children` and `ref`. */
type SvgShapeElementProps<Native> = Native & {
  key?: Key | null
  children?: ReactNode
  ref?: Ref<PublicInstance>
}

export type SvgPathProps = SvgShapeElementProps<NativeSvgPathProps>
export type SvgCircleProps = SvgShapeElementProps<NativeSvgCircleProps>
export type SvgEllipseProps = SvgShapeElementProps<NativeSvgEllipseProps>
export type SvgRectProps = SvgShapeElementProps<NativeSvgRectProps>
export type SvgLineProps = SvgShapeElementProps<NativeSvgLineProps>
export type SvgPolyProps = SvgShapeElementProps<NativeSvgPolyProps>
export type SvgGroupProps = SvgShapeElementProps<NativeSvgGroupProps>
export type SvgDefsProps = SvgShapeElementProps<NativeSvgDefsProps>
export type SvgGradientProps = SvgShapeElementProps<NativeSvgGradientProps>
export type SvgStopProps = SvgShapeElementProps<NativeSvgStopProps>
export type SvgClipPathProps = SvgShapeElementProps<NativeSvgClipPathProps>
export type SvgMaskProps = SvgShapeElementProps<NativeSvgMaskProps>
export type SvgUseProps = SvgShapeElementProps<NativeSvgUseProps>
export type SvgSymbolProps = SvgShapeElementProps<NativeSvgSymbolProps>
export type SvgPatternProps = SvgShapeElementProps<NativeSvgPatternProps>
export type SvgMarkerProps = SvgShapeElementProps<NativeSvgMarkerProps>
export type SvgImageProps = SvgShapeElementProps<NativeSvgImageProps>
export type SvgTspanProps = SvgShapeElementProps<NativeSvgTspanProps>
export type SvgTextPathProps = SvgShapeElementProps<NativeSvgTextPathProps>
export type SvgFilterProps = SvgShapeElementProps<NativeSvgFilterProps>
export type SvgFilterPrimitiveProps = SvgShapeElementProps<NativeSvgFilterPrimitiveProps>
export type SvgFeBlendProps = SvgShapeElementProps<NativeSvgFeBlendProps>
export type SvgFeColorMatrixProps = SvgShapeElementProps<NativeSvgFeColorMatrixProps>
export type SvgFeComponentTransferProps = SvgShapeElementProps<NativeSvgFeComponentTransferProps>
export type SvgFeFuncProps = SvgShapeElementProps<NativeSvgFeFuncProps>
export type SvgFeCompositeProps = SvgShapeElementProps<NativeSvgFeCompositeProps>
export type SvgFeConvolveMatrixProps = SvgShapeElementProps<NativeSvgFeConvolveMatrixProps>
export type SvgFeDiffuseLightingProps = SvgShapeElementProps<NativeSvgFeDiffuseLightingProps>
export type SvgFeDisplacementMapProps = SvgShapeElementProps<NativeSvgFeDisplacementMapProps>
export type SvgFeDistantLightProps = SvgShapeElementProps<NativeSvgFeDistantLightProps>
export type SvgFeDropShadowProps = SvgShapeElementProps<NativeSvgFeDropShadowProps>
export type SvgFeFloodProps = SvgShapeElementProps<NativeSvgFeFloodProps>
export type SvgFeGaussianBlurProps = SvgShapeElementProps<NativeSvgFeGaussianBlurProps>
export type SvgFeImageProps = SvgShapeElementProps<NativeSvgFeImageProps>
export type SvgFeMergeProps = SvgShapeElementProps<NativeSvgFeMergeProps>
export type SvgFeMergeNodeProps = SvgShapeElementProps<NativeSvgFeMergeNodeProps>
export type SvgFeMorphologyProps = SvgShapeElementProps<NativeSvgFeMorphologyProps>
export type SvgFeOffsetProps = SvgShapeElementProps<NativeSvgFeOffsetProps>
export type SvgFePointLightProps = SvgShapeElementProps<NativeSvgFePointLightProps>
export type SvgFeSpecularLightingProps = SvgShapeElementProps<NativeSvgFeSpecularLightingProps>
export type SvgFeSpotLightProps = SvgShapeElementProps<NativeSvgFeSpotLightProps>
export type SvgFeTileProps = SvgShapeElementProps<NativeSvgFeTileProps>
export type SvgFeTurbulenceProps = SvgShapeElementProps<NativeSvgFeTurbulenceProps>

/**
 * `<text>` is the host text element too, so its props are the host ones plus
 * the SVG attributes the host element does not have.
 */
export type SvgTextProps = Props & Omit<NativeSvgTextProps, keyof Props>

export interface CodeProps extends ReactHostProps<NativeCodeProps> {
  key?: Key | null
  children?: ReactNode
  ref?: Ref<PublicInstance>
}

export interface DiffProps extends ReactHostProps<NativeDiffProps> {
  key?: Key | null
  children?: ReactNode
  ref?: Ref<PublicInstance>
}

export interface MarkdownProps extends ReactHostProps<NativeMarkdownProps> {
  key?: Key | null
  children?: ReactNode
  ref?: Ref<PublicInstance>
}

export interface AnchoredProps extends ReactHostProps<NativeAnchoredProps> {
  key?: Key | null
  children?: ReactNode
  ref?: Ref<PublicInstance>
}

export type VirtualListProps = import("@gpuix/native/host").VirtualListProps & {
  key?: Key | null
  children?: ReactNode
  ref?: Ref<PublicInstance>
}

export interface Container {
  nativeRenderer: NativeRenderer
  renderer: MutationRenderer
  ids: ElementIdAllocator
  eventHandlers: EventHandlerMap
  /** Element id to the motion values its style binds, released when the element is destroyed. */
  motionBindings: Map<number, MotionBinding>
  /** Releases waiting for the mutation batch that stops the elements from reading the value. */
  motionReleases: Array<() => void>
  /** Mounted host elements by id, for `document.activeElement`. A portal container shares its root's. */
  instances: Map<number, Instance>
  windowKeyEventHandlers: WindowKeyEventHandlers
  windowKeyEventId: number
  windowSelectionEventId: number
  windowAppearanceEventId: number
  onEvent?: (event: EventPayload) => void
  /** Where an error native raises while applying a commit's mutations goes. */
  onUncaughtError: (error: Error, errorInfo: { componentStack?: string }) => void
  /**
   * Set on a portal's container. Its children mount under this element instead
   * of becoming the window's root. The container inherits everything else from
   * the root container it was made from, so both share one mutation queue and
   * one event map.
   */
  portalTarget?: Instance
}

/** Host methods are attached when React hands out a ref, not per instance. */
export interface Instance extends Partial<FocusableElement> {
  id: number
  type: ElementType
  props: Props
  /** The element this one is placed in; absent at the root of a window. */
  parentId?: number
  setPointerCapture?(pointerId: number): void
  releasePointerCapture?(pointerId: number): void
  hasPointerCapture?(pointerId: number): boolean
  click?(): void
  requestPointerLock?(): void
  /**
   * The border box in window coordinates, like the DOM. Pending changes are
   * sent and laid out first, so it is never stale. An element with no box
   * (not painted) reports a zero rect.
   */
  getBoundingClientRect?(): BoundingClientRect
  [HOST_BINDING]?(): HostBinding | undefined
}

export interface ImgInstance extends PublicInstance {
  type: "img"
  setImage(bytes: Buffer | Uint8Array): void
  setImagePixels(
    width: number,
    height: number,
    pixels: Buffer | Uint8Array,
    options?: ImagePixelsOptions,
  ): void
}

/** The ref of an `<input>` or `<textarea>`: the DOM selection API. */
export interface TextControlInstance extends PublicInstance, TextControlSelection {
  type: "input" | "textarea"
}

/** The ref of a `<form>`. */
export interface FormInstance extends PublicInstance, FormMethods {
  type: "form"
}

export interface TextInstance {
  id: number
  text: string
  parentId: number | null
}

/** What a `ref` receives: the methods are attached before a ref sees the instance. */
export type PublicInstance = Instance &
  FocusableElement &
  ScrollableElement &
  Required<Pick<Instance, "getBoundingClientRect" | typeof HOST_BINDING>>

export interface HostContext {
  isInsideText: boolean
}

export type { ElementIdAllocator, EventHandlerMap, NativeRenderer, RootEventHandlers }
