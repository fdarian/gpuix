/// Host config for React's reconciler — mutation-based protocol.
///
/// Each reconciler callback (createInstance, appendChild, commitUpdate, etc.)
/// makes a direct napi call to the Rust retained tree. No JSON serialization
/// of the full element tree. Only changed elements cross the FFI boundary.

import { createContext } from "react"
import { DefaultEventPriority } from "react-reconciler/constants.js"
import type { HostEventHandler } from "@ghia/native/host"
import {
  assertSvgPlacement,
  createElementMethods,
  defineScrollMembers,
  blurHostElement,
  createWebviewMethods,
  eventTypesWithOverrides,
  focusHostElement,
  HOST_BINDING,
  hostElementMatches,
  isReservedProp,
  readBoundingClientRect,
  sameMutationValue,
  serializeCustomProp,
  forwardsCustomProp,
  readTextControlSelection,
  requestFormSubmit,
  selectTextControl,
  setTextControlSelectionRange,
  splitMotionStyle,
  syncNativeviewListeners,
} from "@ghia/native/host"

const NoEventPriority = 0

/**
 * React DOM listens to `focusin` and `focusout` for `onFocus` and `onBlur`, so
 * they bubble. React has no `onFocusIn` or `onFocusOut`.
 */
const EVENT_TYPE_BY_PROP = eventTypesWithOverrides({
  onFocus: "focusin",
  onBlur: "focusout",
  onFocusIn: null,
  onFocusOut: null,
  // A `<nativeview>`'s handlers take the pointer and nothing, not an event: see `syncNativeviewListeners`.
  onAttach: null,
  onDetach: null,
})
import type {
  Container,
  ElementType,
  FormInstance,
  HostContext,
  ImgInstance,
  Instance,
  MutationRenderer,
  Props,
  PublicInstance,
  TextControlInstance,
  TextInstance,
  WebviewInstance,
} from "../types/host.js"
import { syncMotionBindings } from "./motion-bindings.js"
import {
  registerEventListener,
  unregisterEventListener,
  unregisterEventHandlers,
} from "./event-registry.js"

let currentUpdatePriority = NoEventPriority

type HostNode = Instance | TextInstance

interface HostNodeState {
  container: Container
  initialChildren?: HostNode[]
  mounted: boolean
  /**
   * React hid this element (Suspense, Activity). Its native style carries
   * `visibility: hidden`, so a style update must keep that rather than
   * overwrite it with the visible one.
   */
  hidden: boolean
}

const hostNodeStates = new WeakMap<HostNode, HostNodeState>()

function isPublicInstance(instance: Instance): instance is PublicInstance {
  return Boolean(instance.focus && instance.blur && instance.matches)
}

function attachHostMethods(instance: Instance): PublicInstance {
  if (isPublicInstance(instance)) return instance
  const state = hostNodeStates.get(instance)
  if (!state) {
    throw new Error(`Host element ${instance.id} was handed out before it was mounted.`)
  }
  const container = state.container
  defineScrollMembers(instance, () => ({
    renderer: container.nativeRenderer,
    id: instance.id,
  }))
  Object.assign(instance, createElementMethods(container.nativeRenderer, instance.id))
  instance.focus = (options) => focusHostElement(container.nativeRenderer, instance.id, options)
  instance.blur = () => blurHostElement(container.nativeRenderer, instance.id)
  instance.matches = (selector) =>
    hostElementMatches(container.nativeRenderer, instance.id, selector)
  instance.getBoundingClientRect = () =>
    readBoundingClientRect(container.nativeRenderer, instance.id)
  instance[HOST_BINDING] = () =>
    state.mounted
      ? { renderer: container.nativeRenderer, mutations: container.renderer }
      : undefined
  if (instance.type === "input" || instance.type === "textarea") {
    attachTextControlMethods(instance as TextControlInstance, container)
  } else if (instance.type === "form") {
    const form = instance as FormInstance
    form.requestSubmit = () =>
      requestFormSubmit({
        id: form.id,
        nativeRenderer: container.nativeRenderer,
        flushMutations: () => container.renderer.flushMutations(),
      })
  } else if (instance.type === "webview") {
    Object.assign(
      instance as WebviewInstance,
      createWebviewMethods({
        id: instance.id,
        nativeRenderer: container.nativeRenderer,
        flushMutations: () => container.renderer.flushMutations(),
      }),
    )
  } else if (instance.type === "img") {
    const img = instance as ImgInstance
    img.setImage = (bytes) => {
      container.nativeRenderer.setImage?.(img.id, bytes as Buffer)
    }
    img.setImagePixels = (width, height, pixels, options) => {
      container.nativeRenderer.setImagePixels?.(
        img.id,
        width,
        height,
        pixels as Buffer,
        options?.format,
      )
    }
  }
  if (!isPublicInstance(instance)) {
    throw new Error(`Host element ${instance.id} is missing its focus methods.`)
  }
  return instance
}

function attachTextControlMethods(control: TextControlInstance, container: Container): void {
  const host = {
    id: control.id,
    nativeRenderer: container.nativeRenderer,
    flushMutations: () => container.renderer.flushMutations(),
  }
  control.select = () => selectTextControl(host)
  control.setSelectionRange = (start, end, direction) =>
    setTextControlSelectionRange(host, start, end, direction)
  Object.defineProperties(control, {
    selectionStart: { get: () => readTextControlSelection(host).start },
    selectionEnd: { get: () => readTextControlSelection(host).end },
    selectionDirection: { get: () => readTextControlSelection(host).direction },
  })
}

function adopt(parent: Instance | undefined, child: Instance | TextInstance): void {
  if (!("type" in child)) return
  if (parent) child.parentId = parent.id
  else delete child.parentId
}

function stateFor(node: HostNode): HostNodeState {
  const state = hostNodeStates.get(node)
  if (!state) {
    throw new Error(`GPUIX host node ${node.id} does not belong to a root`)
  }
  return state
}

function containerFor(node: HostNode): Container {
  return stateFor(node).container
}

function rendererFor(node: HostNode): MutationRenderer {
  return containerFor(node).renderer
}

function nextId(container: Container): number {
  return ++container.ids.nextElementId
}

/** SVG shapes exist only inside an `<svg>`; text and other elements go anywhere. */
function assertPlacement(parent: Instance | undefined, child: Instance | TextInstance): void {
  if ("type" in child) assertSvgPlacement(parent?.type, child.type)
}

// ── Event wiring helpers ─────────────────────────────────────────────

function syncEventListeners(container: Container, id: number, type: ElementType, props: Props): void {
  const record = props as Record<string, unknown>
  if (type === "nativeview") syncNativeviewProps(container, id, record)
  for (const propName in record) {
    const eventType = EVENT_TYPE_BY_PROP.get(propName)
    const handler = record[propName]
    if (eventType && handler) {
      registerEventListener(
        container.eventHandlers,
        container.renderer,
        id,
        eventType,
        handler as HostEventHandler
      )
    }
  }
}

function syncNativeviewProps(
  container: Container,
  id: number,
  props: Record<string, unknown>
): void {
  syncNativeviewListeners(
    container.eventHandlers,
    container.renderer,
    id,
    props.onAttach,
    props.onDetach
  )
}

function diffNativeviewProps(
  container: Container,
  id: number,
  oldProps: Props,
  newProps: Props
): void {
  const previous = oldProps as Record<string, unknown>
  const next = newProps as Record<string, unknown>
  if (previous.onAttach === next.onAttach && previous.onDetach === next.onDetach) return
  syncNativeviewProps(container, id, next)
}

/**
 * Walks the props an element has, not every event GPUIX knows: this runs for
 * each re-rendered host element, and most have a few props and no handlers.
 */
function diffEventListeners(
  container: Container,
  id: number,
  oldProps: Props,
  newProps: Props
): void {
  const previous = oldProps as Record<string, unknown>
  const next = newProps as Record<string, unknown>
  for (const propName in next) {
    const eventType = EVENT_TYPE_BY_PROP.get(propName)
    if (!eventType) continue
    const oldHandler = previous[propName]
    const newHandler = next[propName]
    if (oldHandler && !newHandler) {
      unregisterEventListener(container.eventHandlers, container.renderer, id, eventType)
    } else if (newHandler && newHandler !== oldHandler) {
      registerEventListener(
        container.eventHandlers,
        container.renderer,
        id,
        eventType,
        newHandler as HostEventHandler
      )
    }
  }
  for (const propName in previous) {
    if (propName in next) continue
    const eventType = EVENT_TYPE_BY_PROP.get(propName)
    if (eventType && previous[propName]) {
      unregisterEventListener(container.eventHandlers, container.renderer, id, eventType)
    }
  }
}

// ── Style helper ─────────────────────────────────────────────────────

const NO_STYLE = {}

function sendStyle(renderer: MutationRenderer, id: number, props: Props): void {
  const style = splitMotionStyle(props.style).style
  if (style == null || Object.keys(style).length === 0) return
  renderer.setStyle(id, style)
}

/**
 * Whether two `style` props reach Rust as the same payload. An absent style and
 * an empty one are the same, and so are two objects holding the same values
 * (a motion value counts as the same only when it is the same value, as it is
 * bound by identity):
 * `style={{ ... }}` is a new object on every render, so reference equality
 * alone would resend every styled element on every commit.
 *
 * Like React DOM, this does not see a style object mutated in place and passed
 * again. Pass a new object when a style changes.
 */
function sameStyle(left: Props["style"], right: Props["style"]): boolean {
  return left === right || sameMutationValue(left ?? NO_STYLE, right ?? NO_STYLE)
}

/** The style a hidden element keeps: its own, minus the states, plus `visibility: hidden`. */
function hiddenStyle(style: Props["style"]) {
  // Hover and active go, because a hidden element must stay hidden. A hover
  // style that sets `visibility` would otherwise paint an element React
  // asked to hide.
  const { hover: _hover, active: _active, ...base } = splitMotionStyle(style).style ?? {}
  return { ...base, visibility: "hidden" }
}

// ── Custom prop forwarding ───────────────────────────────────────────

// Props that are handled by the reconciler directly (not forwarded as custom props).
/** Send all custom props to Rust for non-built-in element types. */
function syncCustomProps(
  renderer: MutationRenderer,
  id: number,
  type: string,
  props: Props
): void {
  for (const [key, value] of Object.entries(props)) {
    if (isReservedProp(key)) continue
    if (!forwardsCustomProp(type, key)) continue
    renderer.setCustomProp(id, key, serializeCustomProp(value))
  }
}

/**
 * Diff and send changed custom props to Rust.
 *
 * Compared by value after serialization: a handler or an inline object is new on
 * every render, and a function serializes to `null` either way, so resending it
 * would only make Rust parse and discard the same value again.
 */
function diffCustomProps(
  renderer: MutationRenderer,
  id: number,
  type: string,
  oldProps: Props,
  newProps: Props
): void {
  const previous = oldProps as Record<string, unknown>
  const next = newProps as Record<string, unknown>
  // Updated or added props
  for (const key in next) {
    if (isReservedProp(key)) continue
    if (!forwardsCustomProp(type, key)) continue
    const value = next[key]
    const oldValue = previous[key]
    if (value === oldValue) continue
    const serialized = serializeCustomProp(value as Parameters<typeof serializeCustomProp>[0])
    if (
      key in previous &&
      sameMutationValue(
        serializeCustomProp(oldValue as Parameters<typeof serializeCustomProp>[0]),
        serialized
      )
    ) {
      continue
    }
    renderer.setCustomProp(id, key, serialized)
  }
  // Removed props
  for (const key in previous) {
    if (isReservedProp(key)) continue
    if (!forwardsCustomProp(type, key)) continue
    if (!(key in next)) renderer.setCustomProp(id, key, null)
  }
}

/**
 * Materialize a render-phase host node only after React places its subtree in
 * the commit phase. Abandoned concurrent renders stay as collectable JS
 * objects and never enter the native mutation queue.
 */
function materialize(node: HostNode): HostNodeState {
  const state = stateFor(node)
  if (state.mounted) return state

  const renderer = state.container.renderer
  if ("type" in node) {
    renderer.createElement(node.id, node.type)
    state.container.instances.set(node.id, node)
    if (state.hidden) renderer.setStyle(node.id, hiddenStyle(node.props.style))
    else sendStyle(renderer, node.id, node.props)
    syncMotionBindings(state.container, node.id, node.props.style, node.type)
    syncEventListeners(state.container, node.id, node.type, node.props)
    syncCustomProps(renderer, node.id, node.type, node.props)
  } else {
    renderer.createElement(node.id, "text")
    renderer.setText(node.id, node.text)
  }
  state.mounted = true

  if (state.initialChildren) {
    for (const child of state.initialChildren) {
      materialize(child)
      renderer.appendChild(node.id, child.id)
    }
    state.initialChildren = undefined
  }
  return state
}

/** What a `ref` gives for the mounted element `id`, or `undefined` once it is gone. */
export function publicInstanceById(container: Container, id: number): PublicInstance | undefined {
  const instance = container.instances.get(id)
  return instance && attachHostMethods(instance)
}

const portalContainers = new WeakMap<Instance, Container>()

/**
 * The container a portal into `target` mounts through.
 *
 * Cached per target because React compares a portal's `containerInfo` by
 * identity, and a new object on every render would unmount and remount the
 * portal's whole subtree.
 */
export function portalContainerFor(target: Instance): Container {
  let container = portalContainers.get(target)
  if (!container) {
    container = Object.assign(Object.create(stateFor(target).container) as Container, {
      portalTarget: target,
    })
    portalContainers.set(target, container)
  }
  return container
}

// ── Host config ──────────────────────────────────────────────────────

export const hostConfig = {
  supportsMutation: true,
  supportsPersistence: false,
  supportsHydration: false,

  // React creates host nodes while rendering and may abandon that work in
  // concurrent mode. Keep the description in JS; materialize it only from a
  // commit-phase placement callback.
  createInstance(
    type: ElementType,
    props: Props,
    rootContainerInstance: Container,
    _hostContext: HostContext
  ): Instance {
    const instance: Instance = {
      id: nextId(rootContainerInstance),
      type,
      props,
    }
    hostNodeStates.set(instance, {
      container: rootContainerInstance,
      mounted: false,
      hidden: false,
    })
    return instance
  },

  appendChild(parent: Instance, child: Instance | TextInstance): void {
    assertPlacement(parent, child)
    const parentState = materialize(parent)
    materialize(child)
    adopt(parent, child)
    parentState.container.renderer.appendChild(parent.id, child.id)
  },

  // React only calls this from the deletion path, never to move a node, so the
  // child is gone for good and has to be freed here. Detaching alone leaked
  // every removed text node: `detachDeletedInstance` runs for host components
  // only, so nothing else would ever destroy a `HostText`.
  removeChild(parent: Instance, child: Instance | TextInstance): void {
    const container = containerFor(parent)
    const destroyed = container.renderer.destroyElement(child.id)
    for (const id of destroyed) {
      unregisterEventHandlers(container.eventHandlers, id)
    }
  },

  insertBefore(
    parent: Instance,
    child: Instance | TextInstance,
    beforeChild: Instance | TextInstance
  ): void {
    assertPlacement(parent, child)
    const parentState = materialize(parent)
    materialize(child)
    adopt(parent, child)
    parentState.container.renderer.insertBefore(parent.id, child.id, beforeChild.id)
  },

  // A window's root has one child, so there is nothing to order. A portal's
  // children share their target with whatever else it holds, and React places
  // one before another whenever their keys reorder.
  insertInContainerBefore(
    container: Container,
    child: Instance,
    beforeChild: Instance
  ): void {
    const target = container.portalTarget
    if (!target) return
    assertPlacement(target, child)
    materialize(child)
    adopt(target, child)
    container.renderer.insertBefore(target.id, child.id, beforeChild.id)
  },

  removeChildFromContainer(parent: Container, child: Instance): void {
    const destroyed = parent.renderer.destroyElement(child.id)
    for (const id of destroyed) {
      unregisterEventHandlers(parent.eventHandlers, id)
    }
  },

  prepareForCommit(_containerInfo: Container): Record<string, unknown> | null {
    return null
  },

  // Batch flush point: flushMutations() sends all queued mutations to Rust
  // in a single applyBatch() FFI call. This is the end of React's synchronous
  // commit phase — all mutations from this render are flushed together.
  // An error thrown here is not caught per fiber like the rest of the commit:
  // React leaves its `isFlushingWork` flag set, and from then on no root in the
  // process commits a synchronous update. Report it as the root's uncaught
  // error instead, like a component's.
  resetAfterCommit(containerInfo: Container): void {
    try {
      containerInfo.renderer.flushMutations()
    } catch (error) {
      containerInfo.onUncaughtError(error instanceof Error ? error : new Error(String(error)), {})
    }
  },

  getRootHostContext(_rootContainerInstance: Container): HostContext {
    return { isInsideText: false }
  },

  getChildHostContext(
    parentHostContext: HostContext,
    type: ElementType,
    _rootContainerInstance: Container
  ): HostContext {
    const isInsideText = type === "text" || type === "span"
    return { ...parentHostContext, isInsideText }
  },

  shouldSetTextContent(_type: ElementType, _props: Props): boolean {
    return false
  },

  createTextInstance(
    text: string,
    rootContainerInstance: Container,
    _hostContext: HostContext
  ): TextInstance {
    const instance: TextInstance = {
      id: nextId(rootContainerInstance),
      text,
      parentId: null,
    }
    hostNodeStates.set(instance, {
      container: rootContainerInstance,
      mounted: false,
      hidden: false,
    })
    return instance
  },

  scheduleTimeout: setTimeout,
  cancelTimeout: clearTimeout,
  noTimeout: -1,

  shouldAttemptEagerTransition(): boolean {
    return false
  },

  finalizeInitialChildren(
    _instance: Instance,
    _type: ElementType,
    _props: Props,
    _rootContainerInstance: Container,
    _hostContext: HostContext
  ): boolean {
    return false
  },

  commitMount(
    _instance: Instance,
    _type: ElementType,
    _props: Props,
    _internalInstanceHandle: unknown
  ): void {},

  commitUpdate(
    instance: Instance,
    _type: ElementType,
    oldProps: Props,
    newProps: Props,
    _internalInstanceHandle: unknown
  ): void {
    const state = stateFor(instance)
    const { container } = state
    // `commitUpdate` runs for every host element whose props object is new,
    // which is every element a parent re-rendered. Most of those did not change
    // their style, so send it only when it did.
    if (!sameStyle(oldProps.style, newProps.style)) {
      container.renderer.setStyle(
        instance.id,
        state.hidden ? hiddenStyle(newProps.style) : (splitMotionStyle(newProps.style).style ?? {})
      )
      syncMotionBindings(container, instance.id, newProps.style, instance.type)
    }
    diffEventListeners(container, instance.id, oldProps, newProps)
    if (instance.type === "nativeview") {
      diffNativeviewProps(container, instance.id, oldProps, newProps)
    }
    // Custom prop diff (for non-div/text elements)
    diffCustomProps(container.renderer, instance.id, instance.type, oldProps, newProps)
    instance.props = newProps
  },

  commitTextUpdate(
    textInstance: TextInstance,
    _oldText: string,
    newText: string
  ): void {
    rendererFor(textInstance).setText(textInstance.id, newText)
    textInstance.text = newText
  },

  appendChildToContainer(container: Container, child: Instance): void {
    const target = container.portalTarget
    assertPlacement(target, child)
    materialize(child)
    adopt(target, child)
    if (target) {
      materialize(target)
      container.renderer.appendChild(target.id, child.id)
    } else {
      container.renderer.setRoot(child.id)
    }
  },

  appendInitialChild(parent: Instance, child: Instance | TextInstance): void {
    assertPlacement(parent, child)
    adopt(parent, child)
    const state = stateFor(parent)
    state.initialChildren ??= []
    state.initialChildren.push(child)
  },

  hideInstance(instance: Instance): void {
    // Keep the element's own style. `visibility: hidden` skips the paint and
    // keeps the layout box, so replacing the whole style here would collapse
    // the box and lose every other style on the element.
    stateFor(instance).hidden = true
    rendererFor(instance).setStyle(instance.id, hiddenStyle(instance.props.style))
  },

  unhideInstance(instance: Instance, props: Props): void {
    stateFor(instance).hidden = false
    rendererFor(instance).setStyle(instance.id, splitMotionStyle(props.style).style ?? {})
  },

  hideTextInstance(_textInstance: TextInstance): void {},
  unhideTextInstance(_textInstance: TextInstance, _text: string): void {},

  clearContainer(_container: Container): void {},

  setCurrentUpdatePriority(newPriority: number): void {
    currentUpdatePriority = newPriority
  },

  getCurrentUpdatePriority: (): number => currentUpdatePriority,

  resolveUpdatePriority(): number {
    if (currentUpdatePriority !== NoEventPriority) {
      return currentUpdatePriority
    }
    return DefaultEventPriority
  },

  maySuspendCommit(): boolean {
    return false
  },

  NotPendingTransition: null,
  HostTransitionContext: createContext(null),
  resetFormInstance(): void {},
  requestPostPaintCallback(): void {},
  trackSchedulerEvent(): void {},

  resolveEventType(): null {
    return null
  },

  resolveEventTimeStamp(): number {
    return -1.1
  },

  preloadInstance(): boolean {
    return true
  },

  startSuspendingCommit(): void {},
  suspendInstance(): void {},

  waitForCommitToBeReady(): null {
    return null
  },

  detachDeletedInstance(instance: Instance): void {
    const container = containerFor(instance)
    const destroyed = container.renderer.destroyElement(instance.id)
    for (const id of destroyed) {
      unregisterEventHandlers(container.eventHandlers, id)
    }
  },

  getPublicInstance(instance: Instance): PublicInstance {
    return attachHostMethods(instance)
  },

  preparePortalMount(_containerInfo: Container): void {},
  isPrimaryRenderer: true,

  getInstanceFromNode(): null {
    return null
  },

  beforeActiveInstanceBlur(): void {},
  afterActiveInstanceBlur(): void {},
  prepareScopeUpdate(): void {},

  getInstanceFromScope(): null {
    return null
  },
}
