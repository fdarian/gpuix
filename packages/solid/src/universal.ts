import { createRoot as createSolidRoot, type JSX } from "solid-js"
import { createRenderer } from "solid-js/universal"
import {
  createHostElement,
  createHostText,
  getFirstChild,
  getNextSibling,
  getParentNode,
  insertHostNode,
  isHostText,
  isHostPropertyValue,
  removeHostNode,
  replaceHostText,
  setHostProperty,
  type HostNode,
  type HostParent,
} from "./host.js"
import { notePresenceHost, runInPresenceScope } from "./presence-scope.js"

const runtime = createRenderer<HostNode | HostParent>({
  createElement: createHostElement,
  createTextNode: createHostText,
  replaceText(node, value) {
    if (!isHostText(node)) throw new TypeError("Expected a GPUIX text node")
    replaceHostText(node, value)
  },
  setProperty(node, name, value, previous) {
    if (
      node.kind !== "root" &&
      isHostPropertyValue(value) &&
      isHostPropertyValue(previous)
    ) {
      setHostProperty(node, name, value, previous)
    }
  },
  insertNode(parent, node, anchor) {
    if (parent.kind === "text" || node.kind === "root" || anchor?.kind === "root") {
      throw new TypeError("Expected GPUIX host nodes")
    }
    insertHostNode(parent, node, anchor)
  },
  isTextNode: isHostText,
  removeNode(parent, node) {
    if (parent.kind === "text" || node.kind === "root") {
      throw new TypeError("Expected GPUIX host nodes")
    }
    removeHostNode(parent, node)
  },
  getParentNode(node) {
    return node.kind === "root" ? undefined : getParentNode(node)
  },
  getFirstChild(node) {
    return node.kind === "text" ? undefined : getFirstChild(node)
  },
  getNextSibling(node) {
    return node.kind === "root" ? undefined : getNextSibling(node)
  },
})

export function universalRender(code: () => JSX.Element, root: HostParent): () => void {
  return createSolidRoot((dispose) => {
    runtime.insert(root, code())
    return dispose
  })
}

export const effect = runtime.effect
export const memo = runtime.memo
export const createComponent: typeof runtime.createComponent = (component, props) =>
  runInPresenceScope(() => runtime.createComponent(component, props))
export const createElement: typeof runtime.createElement = (tag) => {
  const element = runtime.createElement(tag)
  notePresenceHost(element as HostNode)
  return element
}
export const createTextNode = runtime.createTextNode
export const insertNode = runtime.insertNode
export const insert = runtime.insert
export const spread = runtime.spread
export const setProp = runtime.setProp
export const mergeProps = runtime.mergeProps
export const use = runtime.use
