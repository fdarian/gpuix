import type { JSX as SolidJSX } from "solid-js"
import type {
  AnchoredProps,
  CodeProps,
  DiffProps,
  FormProps,
  HostProps,
  ImgProps,
  InputProps,
  LabelProps,
  MarkdownProps,
  MotionProps,
  MotionValueStyle,
  SvgMotionProps,
  SvgCircleProps,
  SvgClipPathProps,
  SvgDefsProps,
  SvgEllipseProps,
  SvgGradientProps,
  SvgGroupProps,
  SvgLineProps,
  SvgPathProps,
  SvgPolyProps,
  SvgProps,
  SvgMaskProps,
  SvgRectProps,
  SvgStopProps,
  SvgSymbolProps,
  SvgUseProps,
  TextareaProps,
  VirtualListProps,
} from "@gpuix/native/host"
import { createComponent, createElement, spread } from "./universal.js"
import type { HostElement } from "./host.js"

export type SolidHostProps<Props = HostProps> = Omit<Props, "style"> & {
  style?: MotionValueStyle
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
    span: SolidHostProps
    img: SolidHostProps<ImgProps>
    svg: SolidHostProps<SvgProps>
    path: SolidHostProps<SvgPathProps>
    circle: SolidHostProps<SvgCircleProps>
    ellipse: SolidHostProps<SvgEllipseProps>
    rect: SolidHostProps<SvgRectProps>
    line: SolidHostProps<SvgLineProps>
    polyline: SolidHostProps<SvgPolyProps>
    polygon: SolidHostProps<SvgPolyProps>
    g: SolidHostProps<SvgGroupProps>
    defs: SolidHostProps<SvgDefsProps>
    linearGradient: SolidHostProps<SvgGradientProps>
    radialGradient: SolidHostProps<SvgGradientProps>
    stop: SolidHostProps<SvgStopProps>
    clipPath: SolidHostProps<SvgClipPathProps>
    mask: SolidHostProps<SvgMaskProps>
    use: SolidHostProps<SvgUseProps>
    symbol: SolidHostProps<SvgSymbolProps>
    canvas: SolidHostProps
    input: SolidHostProps<InputProps>
    textarea: SolidHostProps<TextareaProps>
    label: SolidHostProps<LabelProps>
    form: SolidHostProps<FormProps>
    anchored: SolidHostProps<AnchoredProps>
    code: SolidHostProps<CodeProps>
    diff: SolidHostProps<DiffProps>
    markdown: SolidHostProps<MarkdownProps>
    "virtual-list": SolidHostProps<VirtualListProps>
  }
}

export type { MotionProps, SvgMotionProps }
