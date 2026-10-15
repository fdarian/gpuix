/// GPUIX JSX dev-runtime types — mirrors jsx-runtime.d.ts for development builds.
///
/// React 19's `react/jsx-dev-runtime` exports only `jsxDEV`, so the aliases here
/// must match jsx-dev-runtime.js instead of re-exporting `jsx` and `jsxs`.

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

export { jsxDEV, jsxDEV as jsx, jsxDEV as jsxs, Fragment } from "react/jsx-dev-runtime"

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
