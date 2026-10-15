/// What the `layout`, `layoutId` and `LayoutGroup` of a `motion.*` component
/// mean to the adapters, shared by React and Solid: how group ids scope a
/// `layoutId`, and the `layout` custom prop the renderer reads.

import type { MotionTransition } from "./host.js"

/** `true` and `"position"` glide the box to its new position. `"size"` snaps, as nothing can scale a box; it still moves a box that holds a `layoutId`. */
export type LayoutMode = boolean | "position" | "size"

export interface MotionLayoutProps {
  /** Glide to the new layout position instead of jumping. `false` turns off a `layoutId` too. */
  layout?: LayoutMode
  /** Elements with the same id hand their glide to each other: a newcomer starts from where the last holder was shown. Implies `layout`. */
  layoutId?: string
}

/** A nested group's id continues its parent's, so ids stay unique per path. */
export function joinLayoutGroupId(parent: string | undefined, id: string | undefined): string | undefined {
  if (id === undefined) return parent
  return parent === undefined ? id : `${parent}-${id}`
}

export function scopeLayoutId(groupId: string | undefined, layoutId: string | undefined): string | undefined {
  if (layoutId === undefined || groupId === undefined) return layoutId
  return `${groupId}-${layoutId}`
}

/** The `layout` entry of a transition configures only the layout glide; the motion description rejects it. */
export function withoutLayoutTransition<Transition extends object>(
  transition: Transition | undefined
): Transition | undefined {
  if (transition === undefined || !("layout" in transition)) return transition
  return Object.fromEntries(Object.entries(transition).filter(([key]) => key !== "layout")) as Transition
}

export type LayoutDescriptionOptions = {
  layout: LayoutMode | undefined
  /** Already scoped by the enclosing `LayoutGroup`. */
  layoutId: string | undefined
  /** Wired, with its `layout` entry still in. */
  transition: MotionTransition | undefined
  reduced: boolean
}

/** The value of the renderer's `layout` custom prop, or `undefined` when the element does not glide. */
export function layoutDescription(options: LayoutDescriptionOptions): Record<string, unknown> | undefined {
  if (options.layout === false) return undefined
  if (options.layout === undefined && options.layoutId === undefined) return undefined
  const description: Record<string, unknown> = {
    mode: options.layout === undefined ? true : options.layout,
  }
  if (options.layoutId !== undefined) description.layoutId = options.layoutId
  if (options.transition !== undefined) description.transition = options.transition
  if (options.reduced) description.reducedMotion = true
  return description
}
