import { createContext, createElement, useContext, useMemo } from "react"
import type { ReactElement, ReactNode } from "react"
import { joinLayoutGroupId } from "@gpuix/native/host"

/** The id prefix `motion.*` components below apply to their `layoutId`. */
export const LayoutGroupContext = createContext<string | undefined>(undefined)

export type LayoutGroupProps = {
  /** Prefixes the `layoutId` of every `motion.*` below as `${id}-${layoutId}`, so two groups can reuse an id without sharing a glide. A nested group continues its parent's prefix. */
  id?: string
  children?: ReactNode
}

export function LayoutGroup(props: LayoutGroupProps): ReactElement {
  const parent = useContext(LayoutGroupContext)
  const groupId = useMemo(() => joinLayoutGroupId(parent, props.id), [parent, props.id])
  return createElement(LayoutGroupContext.Provider, { value: groupId }, props.children)
}
