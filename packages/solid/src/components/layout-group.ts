import { createComponent, createContext, useContext, type Accessor, type JSX } from "solid-js"
import { joinLayoutGroupId } from "@gpuix/native/host"

/** The id prefix `motion.*` components below apply to their `layoutId`. */
export const LayoutGroupContext = createContext<Accessor<string | undefined>>()

export type LayoutGroupProps = {
  /** Prefixes the `layoutId` of every `motion.*` below as `${id}-${layoutId}`, so two groups can reuse an id without sharing a glide. A nested group continues its parent's prefix. */
  id?: string
  children?: JSX.Element
}

export function LayoutGroup(props: LayoutGroupProps): JSX.Element {
  const parent = useContext(LayoutGroupContext)
  const groupId = () => joinLayoutGroupId(parent?.(), props.id)
  return createComponent(LayoutGroupContext.Provider, {
    value: groupId,
    get children() {
      return props.children
    },
  })
}
