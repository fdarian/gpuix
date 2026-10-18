import { createContext, type Accessor } from "solid-js"
import type { MotionVariantContext } from "@gpuix/native/host"

/** The variant names the nearest `motion.*` that controls variants hands down, read as they change. */
export const VariantContext = createContext<Accessor<MotionVariantContext | undefined>>()
