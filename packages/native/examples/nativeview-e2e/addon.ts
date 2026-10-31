/** The test addon (`addon/`): the library side of a `<nativeview>`. It is a separate napi module with no GPUI
 *  in it, so a pointer as a `bigint` is the only thing that crosses to it. Built by `build-addon.sh`. */
import { createRequire } from "node:module"

import { ADDON_PATH } from "./paths"

export type Hit = "self" | "nil" | "left-half"

export interface AttachOptions {
  color: string
  alpha?: number
  hit?: Hit
  outside?: boolean
  outsideColor?: string
  acceptsFirstResponder?: boolean
}

export interface Stats {
  mouseDowns: number
  hitTests: number
  attached: boolean
}

export interface LogEntry {
  seq: number
  kind: "attach" | "detach"
  /** The content view pointer in decimal digits. */
  handle: string
  hadSuperview?: boolean
  hadWindow?: boolean
}

interface Addon {
  attach(handle: bigint, options: AttachOptions): void
  detach(handle: bigint): void
  stats(handle: bigint): Stats
  makeFirstResponder(handle: bigint): boolean
  log(): LogEntry[]
}

export const addon = createRequire(import.meta.url)(ADDON_PATH) as Addon
