import * as native from "@ghia/native"
import type { Display } from "@ghia/native"

export type { Display, Rectangle, Size } from "@ghia/native"

// The browser entry has no display functions, so they are read off the
// namespace: a named import would fail the browser bundle of the adapters.
function nativeScreen<Name extends "getAllDisplays" | "getPrimaryDisplay">(name: Name) {
  const read = native[name]
  if (typeof read !== "function") {
    throw new Error(`screen.${name}() is only implemented on macOS`)
  }
  return read
}

/** Readable before any window opens. macOS only: both throw elsewhere. */
export const screen = {
  getAllDisplays: (): Display[] => nativeScreen("getAllDisplays")(),
  getPrimaryDisplay: (): Display => nativeScreen("getPrimaryDisplay")(),
}
