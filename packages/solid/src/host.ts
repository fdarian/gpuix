// Host-tree shape is based on jhomra21/gpuix-solid (MIT), stripped to GPUIX's
// native elements and event contract.
import {
  assertSvgPlacement,
  createElementMethods,
  defineScrollMembers,
  EVENT_PROP_NAMES,
  EVENT_TYPE_BY_PROP,
  blurHostElement,
  focusHostElement,
  hostElementMatches,
  forwardsCustomProp,
  registerEventListener,
  HOST_BINDING,
  HOST_MOUNT,
  readBoundingClientRect,
  toBoundingClientRect,
  retireMotionBindings,
  serializeCustomProp,
  splitMotionStyle,
  syncMotionBindings,
  unregisterEventHandlers,
  unregisterEventListener,
  readTextControlSelection,
  requestFormSubmit,
  selectTextControl,
  setTextControlSelectionRange,
} from "@gpuix/native/host"
import type {
  ElementMethods,
  BoundingClientRect,
  ElementType,
  EventHandlerMap,
  FocusOptions,
  HostBinding,
  HostProps,
  ImagePixelsOptions,
  MotionBinding,
  MotionBindingHost,
  MotionValue,
  MotionValueStyle,
  MutationRenderer,
  NativeRenderer,
  SelectionDirection,
  ScrollableElement,
  StyleDesc,
  TextControlHost,
} from "@gpuix/native/host"
import type { DomEvent } from "@gpuix/native/host"

export type HostNode = HostElement | HostText
export type HostParent = HostRoot | HostElement
export type HostPropertyValue =
  | object
  | string
  | number
  | boolean
  | ((event: DomEvent) => void)
  | null
  | undefined

export function isHostPropertyValue<Value>(
  value: Value
): value is Value & HostPropertyValue {
  const type = typeof value
  return value == null || type === "object" || type === "string" ||
    type === "number" || type === "boolean" || type === "function"
}

export class HostRoot implements MotionBindingHost {
  readonly kind = "root" as const
  readonly children: HostNode[] = []
  readonly motionBindings = new Map<number, MotionBinding>()
  readonly motionReleases: Array<() => void> = []
  /** Nodes taken out of the tree whose element is destroyed at the next flush, unless they are inserted again first. */
  readonly removals = new Set<HostNode>()
  /** Mounted elements by id, for `document.activeElement`. */
  readonly elements = new Map<number, HostElement>()

  readonly mutations: MutationRenderer
  readonly nativeRenderer: NativeRenderer
  readonly eventHandlers: EventHandlerMap
  readonly allocateId: () => number

  constructor(options: {
    mutations: MutationRenderer
    nativeRenderer: NativeRenderer
    eventHandlers: EventHandlerMap
    allocateId: () => number
  }) {
    this.mutations = options.mutations
    this.nativeRenderer = options.nativeRenderer
    this.eventHandlers = options.eventHandlers
    this.allocateId = options.allocateId
  }

  /** Destroys the removed nodes that stayed out of the tree. Call before each flush. */
  finishRemovals(): void {
    const removed = [...this.removals]
    this.removals.clear()
    for (const node of removed) {
      if (node.parent !== null || !node.mounted) continue
      markUnmounted(node)
      this.mutations.destroyElement(node.id)
    }
  }
}

export class HostElement {
  readonly kind = "element" as const
  readonly children: HostNode[] = []
  readonly props = new Map<string, HostPropertyValue>()
  parent: HostParent | null = null
  root: HostRoot | null = null
  id = 0
  mounted = false
  style: StyleDesc = {}
  /**
   * Held on the instance, not in a module map: the JSX runtime can load a second
   * copy of this module, and the copy that mounts the node must see them.
   */
  readonly mountListeners = new Set<() => void>()
  /** The `style` entries that are motion values, bound while the element is mounted. */
  motionValues: Record<string, MotionValue> | undefined

  constructor(readonly type: ElementType) {}

  setPointerCapture(pointerId: number): void {
    this.methods().setPointerCapture(pointerId)
  }

  releasePointerCapture(pointerId: number): void {
    this.methods().releasePointerCapture(pointerId)
  }

  hasPointerCapture(pointerId: number): boolean {
    return this.methods().hasPointerCapture(pointerId)
  }

  click(): void {
    this.methods().click()
  }

  requestPointerLock(): void {
    this.methods().requestPointerLock()
  }

  focus(options?: FocusOptions): void {
    focusHostElement(this.mountedRenderer("focus"), this.id, options)
  }

  blur(): void {
    blurHostElement(this.mountedRenderer("blur"), this.id)
  }

  matches(selector: string): boolean {
    return hostElementMatches(this.mountedRenderer("matches"), this.id, selector)
  }

  private methods(): ElementMethods {
    if (!this.root) throw new Error("The element is not in a root")
    return createElementMethods(this.root.nativeRenderer, this.id)
  }

  private mountedRenderer(method: string): NativeRenderer {
    if (!this.root || !this.mounted) {
      throw new Error(`Cannot call ${method}() on an element that is not mounted`)
    }
    return this.root.nativeRenderer
  }

  /** `<input>` and `<textarea>` only: select all of the text. */
  select(): void {
    selectTextControl(this.textControl("select()"))
  }

  /** `<input>` and `<textarea>` only. Offsets are UTF-16 code units. */
  setSelectionRange(start: number, end: number, direction?: SelectionDirection): void {
    setTextControlSelectionRange(this.textControl("setSelectionRange()"), start, end, direction)
  }

  /** `<input>` and `<textarea>` only. */
  get selectionStart(): number {
    return readTextControlSelection(this.textControl("selectionStart")).start
  }

  /** `<input>` and `<textarea>` only. */
  get selectionEnd(): number {
    return readTextControlSelection(this.textControl("selectionEnd")).end
  }

  /** `<input>` and `<textarea>` only. */
  get selectionDirection(): SelectionDirection {
    return readTextControlSelection(this.textControl("selectionDirection")).direction
  }

  /** `<form>` only: validate the controls, then submit. */
  requestSubmit(): void {
    const root = this.root
    if (!root || !this.mounted) throw new Error("requestSubmit() needs a mounted element")
    if (this.type !== "form") {
      throw new Error(`requestSubmit() is only valid on <form>, got <${this.type}>`)
    }
    requestFormSubmit({
      id: this.id,
      nativeRenderer: root.nativeRenderer,
      flushMutations: () => root.mutations.flushMutations(),
    })
  }

  private textControl(member: string): TextControlHost {
    const root = this.root
    if (!root || !this.mounted) throw new Error(`${member} needs a mounted element`)
    if (this.type !== "input" && this.type !== "textarea") {
      throw new Error(`${member} is only valid on <input> and <textarea>, got <${this.type}>`)
    }
    return {
      id: this.id,
      nativeRenderer: root.nativeRenderer,
      flushMutations: () => root.mutations.flushMutations(),
    }
  }

  /**
   * The border box in window coordinates, like the DOM. Pending changes are
   * sent and laid out first, so it is never stale. An element that is not
   * mounted, or has no box (not painted), reports a zero rect.
   */
  getBoundingClientRect(): BoundingClientRect {
    if (!this.root || !this.mounted) return toBoundingClientRect(null)
    return readBoundingClientRect(this.root.nativeRenderer, this.id)
  }

  [HOST_MOUNT](listener: () => void): () => void {
    this.mountListeners.add(listener)
    return () => this.mountListeners.delete(listener)
  }

  [HOST_BINDING](): HostBinding | undefined {
    if (!this.root || !this.mounted) return undefined
    return { renderer: this.root.nativeRenderer, mutations: this.root.mutations }
  }

  setImage(bytes: Buffer | Uint8Array): void {
    this.root?.nativeRenderer.setImage?.(this.id, bytes as Buffer)
  }

  setImagePixels(
    width: number,
    height: number,
    pixels: Buffer | Uint8Array,
    options?: ImagePixelsOptions
  ): void {
    this.root?.nativeRenderer.setImagePixels?.(
      this.id,
      width,
      height,
      pixels as Buffer,
      options?.format
    )
  }
}

/** The DOM scroll members: `scrollTop`, `scrollTo`, `scrollIntoView` and the rest. */
export interface HostElement extends ScrollableElement {}
defineScrollMembers(HostElement.prototype, (element: HostElement) => ({
  renderer: element.root?.nativeRenderer,
  id: element.id,
}))

export class HostText {
  readonly kind = "text" as const
  readonly children: HostNode[] = []
  parent: HostParent | null = null
  root: HostRoot | null = null
  id = 0
  mounted = false

  constructor(public text: string) {}
}

export function createHostElement(type: string): HostElement {
  if (!isElementType(type)) throw new Error(`Unsupported GPUIX element <${type}>`)
  return new HostElement(type)
}

function isElementType(type: string): type is ElementType {
  switch (type) {
    case "div":
    case "text":
    case "span":
    case "img":
    case "svg":
    case "canvas":
    case "input":
    case "textarea":
    case "label":
    case "form":
    case "anchored":
    case "code":
    case "diff":
    case "markdown":
    case "virtual-list":
    case "path":
    case "circle":
    case "ellipse":
    case "rect":
    case "line":
    case "polyline":
    case "polygon":
    case "g":
    case "defs":
    case "linearGradient":
    case "radialGradient":
    case "stop":
    case "clipPath":
    case "mask":
    case "use":
    case "symbol":
    case "pattern":
    case "marker":
    case "image":
    case "tspan":
    case "textPath":
    case "filter":
    case "feBlend":
    case "feColorMatrix":
    case "feComponentTransfer":
    case "feFuncR":
    case "feFuncG":
    case "feFuncB":
    case "feFuncA":
    case "feComposite":
    case "feConvolveMatrix":
    case "feDiffuseLighting":
    case "feDisplacementMap":
    case "feDistantLight":
    case "feDropShadow":
    case "feFlood":
    case "feGaussianBlur":
    case "feImage":
    case "feMerge":
    case "feMergeNode":
    case "feMorphology":
    case "feOffset":
    case "fePointLight":
    case "feSpecularLighting":
    case "feSpotLight":
    case "feTile":
    case "feTurbulence":
      return true
    default:
      return false
  }
}

export function createHostText(value: string): HostText {
  return new HostText(String(value))
}

export function replaceHostText(node: HostText, value: string): void {
  const next = String(value)
  if (node.text === next) return
  node.text = next
  if (node.root && node.mounted) node.root.mutations.setText(node.id, next)
}

function eventTypeFor(name: string): string | undefined {
  return EVENT_TYPE_BY_PROP.get(name)
}

function customPropAllowed(node: HostElement, name: string): boolean {
  return forwardsCustomProp(node.type, name)
}

export function setHostProperty(
  node: HostNode,
  name: string,
  value: HostPropertyValue,
  previous?: HostPropertyValue
): void {
  if (node.kind === "text" || name === "children" || name === "ref" || name === "key") {
    return
  }
  if (name === "style") {
    const split = splitMotionStyle(isStyle(value) ? value : undefined)
    node.style = split.style ?? {}
    node.motionValues = split.motionValues
    if (node.root && node.mounted) {
      node.root.mutations.setStyle(node.id, node.style)
      syncMotionBindings(node.root, node.root.mutations, node.id, node.motionValues)
    }
    return
  }
  const eventType = eventTypeFor(name)
  if (eventType) {
    if (isEventHandler(value)) node.props.set(name, value)
    else node.props.delete(name)
    if (!node.root || !node.mounted) return
    if (isEventHandler(value)) {
      registerEventListener(
        node.root.eventHandlers,
        node.root.mutations,
        node.id,
        eventType,
        value
      )
    } else if (isEventHandler(previous)) {
      unregisterEventListener(node.root.eventHandlers, node.root.mutations, node.id, eventType)
    }
    return
  }
  if (value === undefined) node.props.delete(name)
  else node.props.set(name, value)
  if (!node.root || !node.mounted || !customPropAllowed(node, name)) return
  node.root.mutations.setCustomProp(
    node.id,
    name,
    serializeCustomProp(value)
  )
}

function mount(root: HostRoot, node: HostNode): void {
  if (node.root && node.root !== root) {
    throw new Error("Cannot move a GPUIX host node between roots")
  }
  node.root = root
  if (!node.id) node.id = root.allocateId()
  if (node.mounted) return
  node.mounted = true
  if (node.kind === "element") root.elements.set(node.id, node)
  root.mutations.createElement(node.id, node.kind === "text" ? "text" : node.type)
  if (node.kind === "text") {
    root.mutations.setText(node.id, node.text)
  } else {
    if (Object.keys(node.style).length > 0) root.mutations.setStyle(node.id, node.style)
    syncMotionBindings(root, root.mutations, node.id, node.motionValues)
    for (const [name, value] of node.props) {
      const eventType = eventTypeFor(name)
      if (eventType && isEventHandler(value)) {
        registerEventListener(root.eventHandlers, root.mutations, node.id, eventType, value)
      } else if (!EVENT_PROP_NAMES.has(name) && customPropAllowed(node, name)) {
        root.mutations.setCustomProp(
          node.id,
          name,
          serializeCustomProp(value)
        )
      }
    }
  }
  if (node.kind === "element") {
    for (const listener of [...node.mountListeners]) listener()
  }
  for (const child of node.children) {
    mount(root, child)
    root.mutations.appendChild(node.id, child.id)
  }
}

function removeFromParent(node: HostNode): void {
  const parent = node.parent
  if (!parent) return
  const index = parent.children.indexOf(node)
  if (index >= 0) parent.children.splice(index, 1)
  node.parent = null
}

export function insertHostNode(
  parent: HostParent,
  node: HostNode,
  requestedAnchor?: HostNode | null
): void {
  // Like the DOM, inserting a node before itself leaves it where it is. Solid's
  // list reconcile does it when it swaps two neighbours.
  const anchor = requestedAnchor === node ? getNextSibling(node) : requestedAnchor
  if (anchor && anchor.parent !== parent) {
    throw new Error("GPUIX Solid anchor is not a child of its parent")
  }
  if (node.kind === "element") {
    assertSvgPlacement(parent.kind === "root" ? undefined : parent.type, node.type)
  }
  removeFromParent(node)
  const index = anchor ? parent.children.indexOf(anchor) : parent.children.length
  parent.children.splice(index, 0, node)
  node.parent = parent
  const root = parent.kind === "root" ? parent : parent.root
  if (!root) return
  root.removals.delete(node)
  mount(root, node)
  if (parent.kind === "root") root.mutations.setRoot(node.id)
  else if (anchor) root.mutations.insertBefore(parent.id, node.id, anchor.id)
  else root.mutations.appendChild(parent.id, node.id)
}

function markUnmounted(node: HostNode): void {
  node.mounted = false
  if (node.root) {
    node.root.elements.delete(node.id)
    unregisterEventHandlers(node.root.eventHandlers, node.id)
    // Retired now, released after the batch: an element mounted again before
    // the flush binds its values afresh, and a shared value is never freed between.
    retireMotionBindings(node.root, [node.id])
  }
  for (const child of node.children) markUnmounted(child)
}

/**
 * Solid moves a node by removing it and inserting it again (`replaceChild`).
 * Destroying the element on removal would recreate it: its focus, pointer
 * capture and layout animation would be gone. It is destroyed when the
 * batch is flushed, unless the node came back.
 */
export function removeHostNode(parent: HostParent, node: HostNode): void {
  if (node.parent !== parent) return
  removeFromParent(node)
  const root = parent.kind === "root" ? parent : parent.root
  if (!root || !node.mounted) return
  root.removals.add(node)
  root.mutations.requestFlush()
}

export function getParentNode(node: HostNode): HostParent | undefined {
  return node.parent ?? undefined
}

export function getFirstChild(parent: HostParent): HostNode | undefined {
  return parent.children[0]
}

export function getNextSibling(node: HostNode): HostNode | undefined {
  const parent = node.parent
  if (!parent) return undefined
  const index = parent.children.indexOf(node)
  return index < 0 ? undefined : parent.children[index + 1]
}

export function isHostText(node: HostNode | HostParent): node is HostText {
  return node.kind === "text"
}

/** Structural, not `instanceof`: a JSX file compiled against the published
 *  package and a component from source build nodes from two class copies. */
export function isHostElement(value: unknown): value is HostElement {
  return typeof value === "object" && value !== null &&
    (value as { kind?: unknown }).kind === "element"
}

function isEventHandler(
  value: HostPropertyValue
): value is (event: DomEvent) => void {
  return typeof value === "function"
}

function isStyle(value: HostPropertyValue): value is StyleDesc {
  return value !== null && typeof value === "object" && !Array.isArray(value)
}

export type SolidHostProps = Omit<HostProps, "style"> & {
  style?: MotionValueStyle
  children?: unknown
  ref?: (element: HostElement) => void
}
