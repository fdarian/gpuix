import type { JSX as SolidJSX } from "solid-js"
import type {
  AnchoredProps,
  CodeProps,
  DiffProps,
  HostProps,
  ImgProps,
  InputProps,
  LabelProps,
  MarkdownProps,
  MotionProps,
  SvgProps,
  TextareaProps,
  VirtualListProps,
} from "@gpuix/native/host"
import { createComponent, createElement, spread } from "./universal.js"
import type { HostElement } from "./host.js"

export type SolidHostProps<Props = HostProps> = Props & {
  children?: SolidJSX.Element
  ref?: ((element: HostElement) => void) | HostElement
}

type Component = (props: Record<string, unknown>) => unknown

function normalizeProps(props: Record<string, unknown> | null | undefined) {
  if (!props || !("key" in props)) return props ?? {}
  const { key: _key, ...rest } = props
  return rest
}

export function jsx(
  type: string | Component,
  props?: Record<string, unknown> | null
): SolidJSX.Element {
  const normalized = normalizeProps(props)
  if (typeof type === "function") {
    return createComponent(type as never, normalized) as never
  }
  const element = createElement(type)
  spread(element, normalized)
  return element as never
}

export const jsxs = jsx
export const jsxDEV = jsx
export function Fragment(props: { children?: unknown }): SolidJSX.Element {
  return (props.children ?? null) as SolidJSX.Element
}

export namespace JSX {
  export type Element = SolidJSX.Element
  export interface ElementChildrenAttribute { children: {} }
  export interface IntrinsicElements {
    div: SolidHostProps
    text: SolidHostProps
    img: SolidHostProps<ImgProps>
    svg: SolidHostProps<SvgProps>
    canvas: SolidHostProps
    input: SolidHostProps<InputProps>
    textarea: SolidHostProps<TextareaProps>
    label: SolidHostProps<LabelProps>
    anchored: SolidHostProps<AnchoredProps>
    code: SolidHostProps<CodeProps>
    diff: SolidHostProps<DiffProps>
    markdown: SolidHostProps<MarkdownProps>
    "virtual-list": SolidHostProps<VirtualListProps>
  }
}

export type { MotionProps }
