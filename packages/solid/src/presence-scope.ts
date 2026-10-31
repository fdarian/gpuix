import { createRoot, getOwner, onCleanup, runWithOwner, type Accessor, type Owner } from "solid-js"
import type { HostNode, HostParent } from "./host.js"

/**
 * Solid disposes the owner of whatever a `Show`, `For` item or `cond && <X/>`
 * stops rendering, and with it every effect of the components inside. An
 * `AnimatePresence` child that is leaving needs those effects alive until its
 * exit is over, so inside an `AnimatePresence` every component runs in a root
 * of its own and the group decides, when the owner that made the component is
 * cleaned up, whether the root goes now or when the exit completes.
 */

export type PresenceRegistration = {
  isPresent: Accessor<boolean>
  safeToRemove(): void
  unregister(): void
}

export interface PresenceScopeGroup {
  /**
   * `owner` is the owner whose cleanup released the scope. True when the group
   * keeps the scope alive and will dispose it itself.
   */
  retain(scope: PresenceScope, owner: Owner): boolean
  /** `waits`: the exit does not finish until `safeToRemove` is called. */
  register(scope: PresenceScope, waits: boolean): PresenceRegistration
}

export type PresenceScope = {
  readonly group: PresenceScopeGroup
  readonly parent: PresenceScope | undefined
  readonly children: Set<PresenceScope>
  /** Host nodes created directly in this scope, in creation order. */
  readonly hosts: { node: HostNode; seq: number }[]
  readonly seq: number
  disposing: boolean
  dispose(): void
}

type OwnerEntry = { group: PresenceScopeGroup; scope: PresenceScope | undefined }

type SharedState = {
  groups: number
  seq: number
  owners: WeakMap<Owner, OwnerEntry>
  hostOwners: WeakMap<HostNode, Owner | null>
}

const SHARED = Symbol.for("@ghia/solid/presence-scope")

/**
 * On `globalThis`: the JSX runtime can load a second copy of this module, and
 * its components and elements must find the groups the first copy made.
 */
function shared(): SharedState {
  const slot = globalThis as typeof globalThis & { [SHARED]?: SharedState }
  slot[SHARED] ??= { groups: 0, seq: 0, owners: new WeakMap(), hostOwners: new WeakMap() }
  return slot[SHARED]
}

function nearest(owner: Owner | null): OwnerEntry | undefined {
  const owners = shared().owners
  for (let current = owner; current; current = current.owner) {
    const entry = owners.get(current)
    if (entry) return entry
  }
  return undefined
}

/** Counts the group while the current owner lives, so components outside any group skip the lookup. */
export function enterPresenceGroup(): void {
  const state = shared()
  state.groups += 1
  onCleanup(() => {
    state.groups -= 1
  })
}

/** Everything the current owner creates, at any depth, belongs to the group. */
export function ownPresenceGroup(group: PresenceScopeGroup): void {
  const owner = getOwner()
  if (owner) shared().owners.set(owner, { group, scope: undefined })
}

export function runInPresenceScope<Result>(run: () => Result): Result {
  const state = shared()
  if (state.groups === 0) return run()
  const owner = getOwner()
  const found = nearest(owner)
  if (!found) return run()
  const parent = found.scope
  return createRoot((dispose) => {
    const root = getOwner()
    if (!root) throw new Error("createRoot has no owner")
    const scope: PresenceScope = {
      group: found.group,
      parent,
      children: new Set(),
      hosts: [],
      seq: ++state.seq,
      disposing: false,
      dispose,
    }
    state.owners.set(root, { group: found.group, scope })
    parent?.children.add(scope)
    if (owner) runWithOwner(owner, () => onCleanup(() => release(scope, owner)))
    return run()
  })
}

function release(scope: PresenceScope, owner: Owner): void {
  scope.parent?.children.delete(scope)
  if (scope.parent?.disposing || !scope.group.retain(scope, owner)) disposePresenceScope(scope)
}

export function disposePresenceScope(scope: PresenceScope): void {
  if (scope.disposing) return
  scope.disposing = true
  scope.parent?.children.delete(scope)
  scope.dispose()
}

export function notePresenceHost(node: HostNode): void {
  const state = shared()
  if (state.groups === 0) return
  const owner = getOwner()
  state.hostOwners.set(node, owner)
  nearest(owner)?.scope?.hosts.push({ node, seq: ++state.seq })
}

/** Whether `owner` is, or is below, the owner that made `node`: cleaning `owner` takes the node out. */
export function madeUnder(node: HostNode, owner: Owner): boolean {
  for (let current = shared().hostOwners.get(node); current; current = current.owner) {
    if (current === owner) return true
  }
  return false
}

export function registerPresence(waits: boolean): PresenceRegistration | undefined {
  const found = nearest(getOwner())
  return found?.scope ? found.group.register(found.scope, waits) : undefined
}

export function isHostNode(value: unknown): value is HostNode {
  return (
    typeof value === "object" &&
    value !== null &&
    "kind" in value &&
    (value.kind === "element" || value.kind === "text")
  )
}

export function isWithin(node: HostNode, ancestor: HostNode): boolean {
  let current: HostNode | HostParent | null = node
  while (current) {
    if (current === ancestor) return true
    current = current.kind === "root" ? null : current.parent
  }
  return false
}

function firstHost(scope: PresenceScope): HostNode | undefined {
  if (scope.hosts.length > 0) return scope.hosts[0]!.node
  for (const child of scope.children) {
    const host = firstHost(child)
    if (host) return host
  }
  return undefined
}

/**
 * A host node that sits where the scope's output does. A scope with no host of
 * its own (a component that renders nothing) takes the last one an ancestor made
 * before it: the element its component was written inside.
 */
export function anchorHost(scope: PresenceScope): HostNode | undefined {
  const own = firstHost(scope)
  if (own) return own
  for (let up = scope.parent; up; up = up.parent) {
    const before = up.hosts.filter((host) => host.seq < scope.seq)
    if (before.length > 0) return before[before.length - 1]!.node
  }
  return undefined
}
