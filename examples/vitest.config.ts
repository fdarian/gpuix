import { applyMacCpuThrottleFromEnv } from '@ghia/react'
import { defineConfig } from 'vitest/config'

applyMacCpuThrottleFromEnv()

export default defineConfig({})
