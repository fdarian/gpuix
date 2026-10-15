/** Framer's `Reorder`: a list the user reorders by dragging its items. */

import { createContext, createElement, useCallback, useContext, useLayoutEffect, useRef, useState } from "react"
import type { ComponentType, ReactElement, ReactNode, Ref } from "react"
import { ReorderController, createReorderDrag } from "@gpuix/native/host"
import type { ReorderAxis } from "@gpuix/native/host"
import { useGpuix, useGpuixRequired } from "../hooks/use-gpuix.js"
import type { Instance, Props } from "../types/host.js"
import { createMotionComponent } from "./index.js"
import type { GpuixComponentType, MotionDragProps } from "./index.js"

const ReorderContext = createContext<ReorderController<unknown> | null>(null)

export type ReorderGroupProps<Value> = Omit<Props, "ref" | "children"> & {
  /** The axis items are laid out and dragged along. */
  axis?: ReorderAxis
  /** The current order. Each value has one `Reorder.Item`. */
  values: Value[]
  /** Called with the order a drag proposes; pass it back as `values`. */
  onReorder: (values: Value[]) => void
  /** The element type of the list. */
  as?: GpuixComponentType
  ref?: Ref<Instance>
  children?: ReactNode
}

function Group<Value>(props: ReorderGroupProps<Value>): ReactElement {
  const gpuix = useGpuix()
  const [controller] = useState(() => new ReorderController<unknown>())
  const axis = props.axis ?? "y"
  const values = props.values
  const onReorder = props.onReorder
  useLayoutEffect(() => {
    controller.configure({
      axis,
      values,
      // The group's items are registered with its own values.
      onReorder: (next) => onReorder(next as Value[]),
      renderer: gpuix.renderer,
    })
  })

  const hostProps: Record<string, unknown> = { ...props }
  delete hostProps.axis
  delete hostProps.values
  delete hostProps.onReorder
  delete hostProps.as
  return createElement(ReorderContext.Provider, { value: controller }, createElement(props.as ?? "div", hostProps))
}

export type ReorderItemProps<Value> = Omit<MotionDragProps, "layout" | "layoutId" | "layoutDrag" | "ref"> & {
  /** Which entry of the group's `values` this item stands for. */
  value: Value
  /** The element type of the item. */
  as?: GpuixComponentType
  ref?: Ref<Instance>
}

const itemComponents = new Map<string, ComponentType<MotionDragProps>>()

function itemComponent(tag: string): ComponentType<MotionDragProps> {
  const known = itemComponents.get(tag)
  if (known) return known
  const created = createMotionComponent<MotionDragProps>(tag) as ComponentType<MotionDragProps>
  itemComponents.set(tag, created)
  return created
}

function setRef(ref: Ref<Instance> | undefined, node: Instance | null): void {
  if (typeof ref === "function") ref(node)
  else if (ref) ref.current = node
}

function Item<Value>(props: ReorderItemProps<Value>): ReactElement {
  const controller = useContext(ReorderContext)
  if (controller === null) throw new Error("Reorder.Item must be used inside a Reorder.Group")
  const renderer = useGpuixRequired()
  const [drag] = useState(createReorderDrag)
  const instance = useRef<Instance | null>(null)
  const userRef = props.ref
  const ref = useCallback(
    (node: Instance | null) => {
      instance.current = node
      setRef(userRef, node)
    },
    [userRef]
  )

  useLayoutEffect(() => {
    const releases = [drag.x, drag.y, drag.active].map((value) => value.bind(renderer))
    return () => {
      for (const release of releases) release()
    }
  }, [drag, renderer])
  const value = props.value
  useLayoutEffect(
    () =>
      controller.register(value, {
        elementId: () => instance.current?.id,
        capturePointer: (pointerId) => {
          if (instance.current?.setPointerCapture === undefined) {
            throw new Error("Reorder.Item: the item has no element to capture the pointer on")
          }
          instance.current.setPointerCapture(pointerId)
        },
        releasePointerCapture: (pointerId) => instance.current?.releasePointerCapture?.(pointerId),
        drag,
      }),
    [controller, value, drag]
  )

  const hostProps: Record<string, unknown> = { ...props }
  delete hostProps.value
  delete hostProps.as
  const onPointerDown = props.onPointerDown
  const onPointerMove = props.onPointerMove
  const onPointerUp = props.onPointerUp
  const onPointerCancel = props.onPointerCancel
  const onLostPointerCapture = props.onLostPointerCapture
  return createElement(itemComponent(props.as ?? "div"), {
    ...hostProps,
    ref,
    layout: true,
    layoutDrag: drag,
    onPointerDown: (event) => {
      onPointerDown?.(event)
      controller.pointerDown(value, event)
    },
    onPointerMove: (event) => {
      onPointerMove?.(event)
      controller.pointerMove(event)
    },
    onPointerUp: (event) => {
      onPointerUp?.(event)
      controller.pointerEnd(value, event)
    },
    onPointerCancel: (event) => {
      onPointerCancel?.(event)
      controller.pointerEnd(value, event)
    },
    onLostPointerCapture: (event) => {
      onLostPointerCapture?.(event)
      controller.pointerEnd(value, event)
    },
  } as MotionDragProps)
}

/**
 * A list the user reorders by dragging its items along `axis`. An item
 * follows the pointer; when its centre passes a neighbour's, `onReorder`
 * gets the swapped order, and the other items glide to their new slots
 * (`layout`). On release the item glides to its slot.
 *
 * ```tsx
 * <Reorder.Group axis="y" values={items} onReorder={setItems}>
 *   {items.map((item) => <Reorder.Item key={item} value={item}>{item}</Reorder.Item>)}
 * </Reorder.Group>
 * ```
 */
export const Reorder = { Group, Item }
