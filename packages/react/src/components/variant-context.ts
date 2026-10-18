import { createContext } from "react"
import type { MotionVariantContext } from "@gpuix/native/host"

/** The variant names the nearest `motion.*` that controls variants hands down. */
export const VariantContext = createContext<MotionVariantContext | undefined>(undefined)
