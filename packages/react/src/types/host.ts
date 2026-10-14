import type { Key, ReactNode, Ref } from "react"
import type { EventPayload } from "@gpuix/native"
import type {
  ElementIdAllocator,
  ElementType,
  EventHandlerMap,
  HostProps,
  ImagePixelsOptions,
  InputProps as NativeInputProps,
  LabelProps as NativeLabelProps,
  TextareaProps as NativeTextareaProps,
  ImgProps as NativeImgProps,
  SvgProps as NativeSvgProps,
  CodeProps as NativeCodeProps,
  DiffProps as NativeDiffProps,
  MarkdownProps as NativeMarkdownProps,
  AnchoredProps as NativeAnchoredProps,
  MutationRenderer,
  NativeRenderer,
  RootEventHandlers,
  TextControlSelection,
  WindowKeyEventHandlers,
} from "@gpuix/native/host"

export * from "@gpuix/native/host"

export interface Props extends HostProps {
  key?: Key | null
  children?: ReactNode
  ref?: Ref<PublicInstance>
}

export interface InputProps extends NativeInputProps {
  key?: Key | null
  children?: ReactNode
  ref?: Ref<TextControlInstance>
}

export interface LabelProps extends NativeLabelProps {
  key?: Key | null
  children?: ReactNode
  ref?: Ref<PublicInstance>
}

export interface TextareaProps extends NativeTextareaProps {
  key?: Key | null
  children?: ReactNode
  ref?: Ref<TextControlInstance>
}

export interface ImgProps extends NativeImgProps {
  key?: Key | null
  children?: ReactNode
  ref?: Ref<ImgInstance>
}

export interface SvgProps extends NativeSvgProps {
  key?: Key | null
  children?: ReactNode
  ref?: Ref<PublicInstance>
}

export interface CodeProps extends NativeCodeProps {
  key?: Key | null
  children?: ReactNode
  ref?: Ref<PublicInstance>
}

export interface DiffProps extends NativeDiffProps {
  key?: Key | null
  children?: ReactNode
  ref?: Ref<PublicInstance>
}

export interface MarkdownProps extends NativeMarkdownProps {
  key?: Key | null
  children?: ReactNode
  ref?: Ref<PublicInstance>
}

export interface AnchoredProps extends NativeAnchoredProps {
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

export interface Instance {
  id: number
  type: ElementType
  props: Props
  scrollIntoView?(): void
  setPointerCapture?(pointerId: number): void
  releasePointerCapture?(pointerId: number): void
  hasPointerCapture?(pointerId: number): boolean
  click?(): void
  requestPointerLock?(): void
}

export interface ImgInstance extends Instance {
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

export interface TextInstance {
  id: number
  text: string
  parentId: number | null
}

export type PublicInstance = Instance

export interface HostContext {
  isInsideText: boolean
}

export type { ElementIdAllocator, EventHandlerMap, NativeRenderer, RootEventHandlers }
