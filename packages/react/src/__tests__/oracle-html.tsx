/// The markup of an oracle page (`fixtures/*-oracle.html`) as the elements it stands for: text and `<span>`s whose
/// `style` attribute is a CSS declaration list.

import React from "react"

/** `text-wrap-mode:nowrap;white-space:pre` as a style object. */
export function styleOf(css: string): Record<string, string> {
  const style: Record<string, string> = {}
  for (const declaration of css.split(";").filter(Boolean)) {
    const [name, value] = declaration.split(":")
    style[name!.trim().replace(/-([a-z])/g, (_, letter: string) => letter.toUpperCase())] = value!.trim()
  }
  return style
}

/** The text of `html` without its tags. */
export function textOf(html: string): string {
  return html.replace(/<[^>]*>/g, "")
}

/** The spans of `html` nested as they are, with the `data-seg` attribute dropped. */
export function nodesOf(html: string): React.ReactNode[] {
  const root: React.ReactNode[] = []
  const open: { style: string; children: React.ReactNode[] }[] = []
  let key = 0
  for (const token of html.split(/(<span(?: [a-z-]+="[^"]*")*>|<\/span>)/).filter(Boolean)) {
    const opening = /^<span((?: [a-z-]+="[^"]*")*)>$/.exec(token)
    if (opening) {
      open.push({ style: /style="([^"]*)"/.exec(opening[1]!)?.[1] ?? "", children: [] })
    } else if (token === "</span>") {
      const span = open.pop()!
      const element = (
        <span key={key++} style={styleOf(span.style) as never}>
          {span.children}
        </span>
      )
      ;(open.at(-1)?.children ?? root).push(element)
    } else {
      ;(open.at(-1)?.children ?? root).push(token)
    }
  }
  return root
}
