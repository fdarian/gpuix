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
  SvgEllipseProps as NativeSvgEllipseProps,
  SvgGroupProps as NativeSvgGroupProps,
  SvgLineProps as NativeSvgLineProps,
  SvgPathProps as NativeSvgPathProps,
  SvgPolyProps as NativeSvgPolyProps,
  SvgRectProps as NativeSvgRectProps,
  CodeProps as NativeCodeProps,
  DiffProps as NativeDiffProps,
  MarkdownProps as NativeMarkdownProps,
  AnchoredProps as NativeAnchoredProps,
  MotionStyle,
  MotionValue,
  MutationRenderer,
  NativeRenderer,
  RootEventHandlers,
  TextControlSelection,
  ScrollableElement,
  StyleDesc,
  WindowKeyEventHandlers,
} from "@gpuix/native/host"

export * from "@gpuix/native/host"

/** `onFocusIn` and `onFocusOut` do not exist in React, where `onFocus` and `onBlur` bubble. */
type ReactHostProps<Props> = Omit<Props, "onFocusIn" | "onFocusOut">

/**
 * `StyleDesc` where each animatable property may hold a motion value instead.
 * The reconciler binds it to the element rather than sending it as style.
 */
export type MotionValueStyle = Omit<StyleDesc, keyof MotionStyle> & {
  [Property in keyof MotionStyle]?: StyleDesc[Property] | MotionValue
}

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
  windowKeyEventHandlers: WindowKeyEventHandlers
  windowKeyEventId: number
  windowSelectionEventId: number
  windowAppearanceEventId: number
  onEvent?: (event: EventPayload) => void
  /**
   * Set on a portal's container. Its children mount under this element instead
   * of becoming the window's root. The container inherits everything else from
   * the root container it was made from, so both share one mutation queue and
   * one event map.
   */
  portalTarget?: Instance
}

export type MotionBinding = {
  values: Record<string, MotionValue>
  releases: Array<() => void>
}

/** Host methods are attached when React hands out a ref, not per instance. */
export interface Instance extends Partial<FocusableElement> {
  id: number
  type: ElementType
  props: Props
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
