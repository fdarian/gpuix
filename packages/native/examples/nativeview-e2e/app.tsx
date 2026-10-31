/** The app the e2e drives: `<nativeview>`s among GPUI content in a real window, with the test addon as
 *  the library that puts a plain coloured NSView into each. One scene at a time; the driver switches them. */
import React, { useEffect, useState } from "react"
import { flushSync, type NativeviewProps } from "@gpuix/react"
import { addon, AttachOptions, Hit } from "./addon"

export const COLORS = {
  page: "#f0f0f0",
  red: "#ff0000",
  green: "#00ff00",
  blue: "#0000ff",
  yellow: "#ffff00",
  magenta: "#ff00ff",
  cyan: "#00ffff",
}

/** Where things are, in points from the top left of the window's content. The driver derives the pixels
 *  it expects from these. Window: 900 x 700. */
export const LAYOUT = {
  window: { width: 900, height: 700 },
  order: {
    translucent: { left: 40, top: 40, width: 200, height: 140 },
    translucentAlpha: 0.5,
    box: { left: 300, top: 40, width: 240, height: 200, border: 14 },
    child: { left: 20, top: 20, size: 40 },
    sibling: { left: 500, top: 200, size: 60 },
  },
  geometry: {
    scroller: { left: 20, top: 20, width: 400, height: 300 },
    scrolled: { spacer: 250, viewLeft: 40, width: 240, height: 120, tail: 800 },
    corner: { right: 30, bottom: 30, width: 160, height: 100 },
    resized: { width: 760, height: 560 },
    transform: {
      wrapper: { left: 480, top: 100, width: 200, height: 120 },
      translate: { x: 60, y: 30 },
      scale: 1.5,
      view: { left: 20, top: 10, width: 100, height: 60 },
    },
    toggle: { left: 480, top: 330, width: 160, height: 100 },
  },
  overflow: {
    box: { left: 100, top: 100, width: 200, height: 140 },
    radius: 40,
    outsideReach: 20,
    ancestor: { left: 60, top: 60, width: 200, height: 150 },
    inAncestor: { left: 40, top: 40, width: 240, height: 160 },
  },
  pointer: {
    box: { left: 100, top: 100, width: 300, height: 200 },
    button: { left: 20, top: 20, width: 80, height: 40 },
    /** A plain div with the same handlers: tells a pointer that does not hover at all from a view that hides it. */
    reference: { left: 500, top: 100, width: 100, height: 60 },
  },
  focus: {
    box: { left: 100, top: 100, width: 300, height: 200 },
    plain: { left: 500, top: 100, width: 100, height: 60 },
    focusable: { left: 500, top: 200, width: 100, height: 60 },
  },
  lifecycle: {
    row: { left: 60, top: 100, width: 500, height: 120, gap: 40 },
    view: { width: 160, height: 120 },
    unmounter: { left: 600, top: 100, width: 80, height: 40 },
  },
}

export type HideMode = "shown" | "display-none" | "visibility-hidden"
export type OverflowMode = "visible" | "hidden" | "visible-round" | "hidden-round" | "ancestor"

export interface PointerScene {
  kind: "pointer"
  hit: Hit
  nativeEvents: boolean
  stopPointerDown: boolean
  pointerEventsNone: boolean
}

export type Scene =
  | { kind: "empty" }
  | { kind: "order" }
  | { kind: "geometry"; toggle: HideMode }
  | { kind: "overflow"; mode: OverflowMode }
  | PointerScene
  | { kind: "focus" }
  | { kind: "lifecycle"; a: boolean; b: boolean; swapped: boolean }

/** What the app saw of one `<nativeview>` by name, across remounts. */
export interface NativeRecord {
  /** The view of the latest attach. */
  handle: bigint | null
  attaches: bigint[]
  detaches: number
  /** What `controls.detachProbe` returned inside each `onDetach`, before the library took its view away. */
  probes: unknown[]
}

export interface Scrollable {
  scrollTop: number
  scrollTo?: (x: number, y: number) => void
}

function freshPointerLog() {
  return {
    pointerDowns: 0,
    clicks: 0,
    buttonClicks: 0,
    hovered: false,
    /** Every enter and leave event the element heard, in order. */
    hoverLog: [] as string[],
    referenceHovered: false,
    /** The library view's `mouseDowns` as each pointerdown handler of the element ran. */
    mouseDownsAtPointerDown: [] as number[],
  }
}

export const controls = {
  records: {} as Record<string, NativeRecord>,
  pointer: freshPointerLog(),
  scroller: null as Scrollable | null,
  /** Asked inside `onDetach`, while the library still holds its view. */
  detachProbe: null as ((name: string) => unknown) | null,
  /** Mounts a scene from scratch: every `<nativeview>` in it is new. */
  show(_scene: Scene) {},
  /** Changes the props of the scene that is mounted. */
  change(_scene: Scene) {},
  resetPointer() {
    controls.pointer = freshPointerLog()
  },
}

export function recordOf(name: string): NativeRecord {
  let record = controls.records[name]
  if (!record) {
    record = { handle: null, attaches: [], detaches: 0, probes: [] }
    controls.records[name] = record
  }
  return record
}

function attachLibrary(name: string, view: bigint, options: AttachOptions) {
  const record = recordOf(name)
  record.handle = view
  record.attaches.push(view)
  addon.attach(view, options)
}

function detachLibrary(name: string) {
  const record = recordOf(name)
  if (record.handle === null) throw new Error(`${name} was detached with no attach before it`)
  if (controls.detachProbe) record.probes.push(controls.detachProbe(name))
  record.detaches += 1
  addon.detach(record.handle)
}

interface ElementHandlers {
  onClick?: () => void
  onPointerDown?: (event: { stopPropagation(): void }) => void
  onPointerEnter?: () => void
  onPointerLeave?: () => void
  onMouseEnter?: () => void
  onMouseLeave?: () => void
}

interface LibraryProps {
  name: string
  options: AttachOptions
  style: NonNullable<NativeviewProps["style"]>
  nativeEvents?: boolean
  handlers?: ElementHandlers
  children?: React.ReactNode
}

function Library(props: LibraryProps) {
  return (
    <nativeview
      nativeEvents={props.nativeEvents}
      style={props.style}
      onAttach={(view: bigint) => attachLibrary(props.name, view, props.options)}
      onDetach={() => detachLibrary(props.name)}
      {...props.handlers}
    >
      {props.children}
    </nativeview>
  )
}

const absolute = (box: { left: number; top: number; width: number; height: number }) => ({
  position: "absolute" as const,
  left: box.left,
  top: box.top,
  width: box.width,
  height: box.height,
})

function OrderScene() {
  const layout = LAYOUT.order
  return (
    <>
      <Library
        name="order-translucent"
        options={{ color: COLORS.red, alpha: layout.translucentAlpha }}
        style={{ ...absolute(layout.translucent), backgroundColor: COLORS.green }}
      />
      <Library
        name="order-box"
        options={{ color: COLORS.red }}
        style={{
          ...absolute(layout.box),
          backgroundColor: COLORS.green,
          border: `${layout.box.border}px solid ${COLORS.magenta}`,
        }}
      >
        <div
          style={{
            ...absolute({ left: layout.child.left, top: layout.child.top, width: layout.child.size, height: layout.child.size }),
            backgroundColor: COLORS.blue,
          }}
        />
      </Library>
      <div
        style={{
          ...absolute({ left: layout.sibling.left, top: layout.sibling.top, width: layout.sibling.size, height: layout.sibling.size }),
          backgroundColor: COLORS.yellow,
        }}
      />
    </>
  )
}

function GeometryScene(props: { toggle: HideMode }) {
  const layout = LAYOUT.geometry
  const scrolled = layout.scrolled
  const transform = layout.transform
  return (
    <>
      <div
        ref={(instance: unknown) => {
          controls.scroller = instance as Scrollable | null
        }}
        style={{ ...absolute(layout.scroller), overflow: "auto", backgroundColor: "#e0e0ff" }}
      >
        <div style={{ height: scrolled.spacer, backgroundColor: "#c0c0ff" }} />
        <Library
          name="scroll"
          options={{ color: COLORS.red }}
          style={{ marginLeft: scrolled.viewLeft, width: scrolled.width, height: scrolled.height }}
        />
        <div style={{ height: scrolled.tail, backgroundColor: "#c0c0ff" }} />
      </div>
      <Library
        name="corner"
        options={{ color: COLORS.red }}
        style={{ position: "absolute", right: layout.corner.right, bottom: layout.corner.bottom, width: layout.corner.width, height: layout.corner.height }}
      />
      <div
        style={{
          ...absolute(transform.wrapper),
          transformOrigin: "0 0",
          transform: `translate(${transform.translate.x}px, ${transform.translate.y}px) scale(${transform.scale})`,
        }}
      >
        <Library name="transform" options={{ color: COLORS.red }} style={absolute(transform.view)} />
      </div>
      <Library
        name="toggle"
        options={{ color: COLORS.red }}
        style={{
          ...absolute(layout.toggle),
          display: props.toggle === "display-none" ? "none" : "block",
          visibility: props.toggle === "visibility-hidden" ? "hidden" : "visible",
        }}
      />
    </>
  )
}

function OverflowScene(props: { mode: OverflowMode }) {
  const layout = LAYOUT.overflow
  const options: AttachOptions = { color: COLORS.red, outside: true, outsideColor: COLORS.cyan }
  if (props.mode === "ancestor") {
    return (
      <div style={{ ...absolute(layout.ancestor), overflow: "hidden" }}>
        <Library name="overflow" options={options} style={{ ...absolute(layout.inAncestor), backgroundColor: COLORS.green }} />
      </div>
    )
  }
  const hidden = props.mode === "hidden" || props.mode === "hidden-round"
  const round = props.mode === "visible-round" || props.mode === "hidden-round"
  return (
    <Library
      name="overflow"
      options={options}
      style={{
        ...absolute(layout.box),
        backgroundColor: COLORS.green,
        overflow: hidden ? "hidden" : "visible",
        borderRadius: round ? layout.radius : 0,
      }}
    />
  )
}

function PointerScene(props: PointerScene) {
  const layout = LAYOUT.pointer
  const handlers: ElementHandlers = {
    onClick: () => {
      controls.pointer.clicks += 1
    },
    onPointerDown: (event) => {
      const record = recordOf("pointer")
      if (record.handle === null) throw new Error("a pointerdown reached the element before its attach")
      controls.pointer.pointerDowns += 1
      controls.pointer.mouseDownsAtPointerDown.push(addon.stats(record.handle).mouseDowns)
      if (props.stopPointerDown) event.stopPropagation()
    },
    onPointerEnter: () => {
      controls.pointer.hovered = true
      controls.pointer.hoverLog.push("pointerenter")
    },
    onPointerLeave: () => {
      controls.pointer.hovered = false
      controls.pointer.hoverLog.push("pointerleave")
    },
    onMouseEnter: () => {
      controls.pointer.hoverLog.push("mouseenter")
    },
    onMouseLeave: () => {
      controls.pointer.hoverLog.push("mouseleave")
    },
  }
  return (
    <>
      <Library
        name="pointer"
        options={{ color: COLORS.red, hit: props.hit }}
        nativeEvents={props.nativeEvents}
        handlers={handlers}
        style={{ ...absolute(layout.box), backgroundColor: COLORS.green, pointerEvents: props.pointerEventsNone ? "none" : "auto" }}
      >
        <div
          onClick={() => {
            controls.pointer.buttonClicks += 1
          }}
          style={{ ...absolute(layout.button), backgroundColor: COLORS.blue, pointerEvents: "auto" }}
        />
      </Library>
      <div
        onPointerEnter={() => {
          controls.pointer.referenceHovered = true
        }}
        onPointerLeave={() => {
          controls.pointer.referenceHovered = false
        }}
        style={{ ...absolute(layout.reference), backgroundColor: COLORS.yellow }}
      />
    </>
  )
}

function FocusScene() {
  const layout = LAYOUT.focus
  return (
    <>
      <Library
        name="focus"
        options={{ color: COLORS.red, hit: "self", acceptsFirstResponder: true }}
        nativeEvents
        style={absolute(layout.box)}
      />
      <div style={{ ...absolute(layout.plain), backgroundColor: COLORS.yellow }} />
      <div tabIndex={0} style={{ ...absolute(layout.focusable), backgroundColor: COLORS.magenta }} />
    </>
  )
}

function LifecycleScene(props: { a: boolean; b: boolean; swapped: boolean }) {
  const layout = LAYOUT.lifecycle
  const items = [
    props.a && (
      <Library key="a" name="life-a" options={{ color: COLORS.red }} style={{ width: layout.view.width, height: layout.view.height }} />
    ),
    props.b && (
      <Library key="b" name="life-b" options={{ color: COLORS.blue }} style={{ width: layout.view.width, height: layout.view.height }} />
    ),
  ]
  return (
    <>
      <div style={{ ...absolute(layout.row), display: "flex", flexDirection: "row", gap: layout.row.gap }}>
        {props.swapped ? items.reverse() : items}
      </div>
      <div
        onClick={() => flushSync(() => controls.change({ kind: "lifecycle", a: false, b: false, swapped: false }))}
        style={{ ...absolute(layout.unmounter), backgroundColor: COLORS.magenta }}
      />
    </>
  )
}

function SceneView(props: { scene: Scene }) {
  const scene = props.scene
  switch (scene.kind) {
    case "empty":
      return null
    case "order":
      return <OrderScene />
    case "geometry":
      return <GeometryScene toggle={scene.toggle} />
    case "overflow":
      return <OverflowScene mode={scene.mode} />
    case "pointer":
      return <PointerScene {...scene} />
    case "focus":
      return <FocusScene />
    case "lifecycle":
      return <LifecycleScene a={scene.a} b={scene.b} swapped={scene.swapped} />
  }
}

interface Current {
  scene: Scene
  /** Changes with every `show`, which remounts the scene. */
  generation: number
}

export function App() {
  const [current, setCurrent] = useState<Current>({ scene: { kind: "empty" }, generation: 0 })

  useEffect(() => {
    controls.show = (scene) => setCurrent((previous: Current) => ({ scene, generation: previous.generation + 1 }))
    controls.change = (scene) => setCurrent((previous: Current) => ({ scene, generation: previous.generation }))
  }, [])

  return (
    <div style={{ position: "relative", width: "100%", height: "100%", backgroundColor: COLORS.page }}>
      <SceneView key={current.generation} scene={current.scene} />
    </div>
  )
}
