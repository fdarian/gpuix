import {
  retireMotionBindings,
  runMotionReleases,
  splitMotionStyle,
  syncMotionBindings as syncBindings,
} from "@gpuix/native/host"
import type { Container, Props } from "../types/host.js"

export function syncMotionBindings(container: Container, id: number, style: Props["style"]): void {
  syncBindings(container, container.renderer, id, splitMotionStyle(style).motionValues)
}

/** Runs once the mutation batch has been applied, with the ids it destroyed. */
export function releaseMotionBindings(container: Container, destroyed: readonly number[]): void {
  retireMotionBindings(container, destroyed)
  runMotionReleases(container)
}
