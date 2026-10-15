/** Framer's `Reorder`: a list the user reorders by dragging its items. */

import {
  createComponent,
  createContext,
  createRenderEffect,
  mergeProps,
  onCleanup,
  splitProps,
  useContext,
  type Component,
  type JSX,
} from "solid-js"
import type { EventPayload } from "@gpuix/native"
import { ReorderController, createReorderDrag } from "@gpuix/native/host"
import type { ReorderAxis } from "@gpuix/native/host"
import type { HostElement } from "../host.js"
import { jsx } from "../jsx-runtime.js"
import type { SolidHostProps } from "../jsx-runtime.js"
import { useGpuixRequired } from "../root.js"
import { createMotionComponent } from "./motion.js"
import type { MotionDragProps } from "./motion.js"

const ReorderContext = createContext<ReorderController<unknown>>()

export type ReorderGroupProps<Value> = Omit<SolidHostProps, "children"> & {
  /** The axis items are laid out and dragged along. */
  axis?: ReorderAxis
  /** The current order. Each value has one `Reorder.Item`. */
  values: Value[]
  /** Called with the order a drag proposes; pass it back as `values`. */
  onReorder: (values: Value[]) => void
  /** The element type of the list. */
  as?: string
  children?: JSX.Element
}

function Group<Value>(props: ReorderGroupProps<Value>): JSX.Element {
  const renderer = useGpuixRequired()
  const controller = new ReorderController<unknown>()
  createRenderEffect(() => {
    controller.configure({
      axis: props.axis ?? "y",
      values: props.values,
      // The group's items are registered with its own values.
      onReorder: (next) => props.onReorder(next as Value[]),
      renderer,
    })
  })
  const [, hostProps] = splitProps(props, ["axis", "values", "onReorder", "as"])
  return createComponent(ReorderContext.Provider, {
    value: controller,
    get children() {
      return jsx(props.as ?? "div", hostProps)
    },
  })
}

export type ReorderItemProps<Value> = Omit<MotionDragProps, "layout" | "layoutId" | "layoutDrag"> & {
  /** Which entry of the group's `values` this item stands for. */
  value: Value
  /** The element type of the item. */
  as?: string
}

const itemComponents = new Map<string, Component<MotionDragProps>>()

function itemComponent(tag: string): Component<MotionDragProps> {
  const known = itemComponents.get(tag)
  if (known) return known
  const created = createMotionComponent<MotionDragProps>(tag)
  itemComponents.set(tag, created)
  return created
}

function Item<Value>(props: ReorderItemProps<Value>): JSX.Element {
  const controller = useContext(ReorderContext)
  if (controller === undefined) throw new Error("Reorder.Item must be used inside a Reorder.Group")
  const renderer = useGpuixRequired()
  const drag = createReorderDrag()
  for (const value of [drag.x, drag.y, drag.active]) onCleanup(value.bind(renderer))

  let element: HostElement | undefined
  createRenderEffect(() => {
    onCleanup(
      controller.register(props.value, {
        elementId: () => (element?.mounted ? element.id : undefined),
        drag,
      })
    )
  })

  const [own, hostProps] = splitProps(props, ["value", "as", "ref", "onMouseDown", "onMouseMove", "onMouseUp"])
  return createComponent(
    itemComponent(own.as ?? "div"),
    mergeProps(hostProps, {
      layout: true,
      layoutDrag: drag,
      ref(node: HostElement) {
        element = node
        if (typeof own.ref === "function") own.ref(node)
      },
      onMouseDown(event: EventPayload) {
        own.onMouseDown?.(event)
        controller.pointerDown(own.value, event)
      },
      onMouseMove(event: EventPayload) {
        own.onMouseMove?.(event)
        controller.pointerMove(event)
      },
      onMouseUp(event: EventPayload) {
        own.onMouseUp?.(event)
        controller.pointerUp()
      },
    })
  )
}

/**
 * A list the user reorders by dragging its items along `axis`. An item
 * follows the pointer; when its centre passes a neighbour's, `onReorder`
 * gets the swapped order, and the other items glide to their new slots
 * (`layout`). On release the item glides to its slot.
 *
 * ```tsx
 * <Reorder.Group axis="y" values={items()} onReorder={setItems}>
 *   <For each={items()}>{(item) => <Reorder.Item value={item}>{item}</Reorder.Item>}</For>
 * </Reorder.Group>
 * ```
 */
export const Reorder = { Group, Item }
