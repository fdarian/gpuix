/// GPUIX JSX runtime types — maps intrinsic elements to GPUIX Props
/// instead of DOM types. Activated via "jsxImportSource": "@gpuix/react".
///
/// `key` is declared on `Props`, not on `IntrinsicAttributes` below.
/// TypeScript 5 ignores `IntrinsicAttributes` for intrinsic elements.

import type * as React from "react"
import type {
  AnchoredProps,
  CodeProps,
  DiffProps,
  FormProps,
  ImgProps,
  InputProps,
  LabelProps,
  MarkdownProps,
  Props,
  SvgCircleProps,
  SvgClipPathProps,
  SvgDefsProps,
  SvgEllipseProps,
  SvgGradientProps,
  SvgGroupProps,
  SvgLineProps,
  SvgMaskProps,
  SvgPathProps,
  SvgPolyProps,
  SvgProps,
  SvgRectProps,
  SvgStopProps,
  SvgSymbolProps,
  SvgUseProps,
  TextareaProps,
  VirtualListProps,
} from "./dist/types/host"

export { jsx, jsxs, Fragment } from "react/jsx-runtime"

export namespace JSX {
  type ElementType = React.JSX.ElementType
  type Element = React.JSX.Element
  type ElementClass = React.JSX.ElementClass
  type ElementAttributesProperty = React.JSX.ElementAttributesProperty
  type ElementChildrenAttribute = React.JSX.ElementChildrenAttribute
  type IntrinsicAttributes = React.JSX.IntrinsicAttributes
  type IntrinsicClassAttributes<T> = React.JSX.IntrinsicClassAttributes<T>

  interface IntrinsicElements {
    div: Props
    text: Props
    span: Props
    img: ImgProps
    svg: SvgProps
    path: SvgPathProps
    circle: SvgCircleProps
    ellipse: SvgEllipseProps
    rect: SvgRectProps
    line: SvgLineProps
    polyline: SvgPolyProps
    polygon: SvgPolyProps
    g: SvgGroupProps
    defs: SvgDefsProps
    linearGradient: SvgGradientProps
    radialGradient: SvgGradientProps
    stop: SvgStopProps
    clipPath: SvgClipPathProps
    mask: SvgMaskProps
    use: SvgUseProps
    symbol: SvgSymbolProps
    canvas: Props
    input: InputProps
    textarea: TextareaProps
    label: LabelProps
    form: FormProps
    anchored: AnchoredProps
    code: CodeProps
    diff: DiffProps
    markdown: MarkdownProps
    "virtual-list": VirtualListProps
  }
}
