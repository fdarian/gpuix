/**
 * Desktop chat example for hermes-node. Always mounts; no Bun entry check.
 */
import React from 'react'
import { applyMacCpuThrottleFromEnv, render } from '@ghia/react'
import { ChatApp } from '../examples/chat.tsx'

applyMacCpuThrottleFromEnv()
render(<ChatApp turnCount={40} />, {
  title: 'GPUIX Chat · Hermes',
  width: 1180,
  height: 820,
  titlebarTransparent: true,
  windowBackground: 'blurred',
  trafficLightX: 16,
  trafficLightY: 17,
  debugFrameOverlay: 'full',
  focus: process.env.GPUIX_BACKGROUND !== '1',
})

const screenshotPath = process.env.GPUIX_SCREENSHOT
if (screenshotPath) {
  setTimeout(() => {
    const slot = globalThis.__gpuixRenderHost as
      | { renderer?: { captureScreenshot?: (path: string) => void; getPaintedText?: () => string[] } }
      | undefined
    const renderer = slot?.renderer
    if (!renderer?.captureScreenshot) {
      console.error('no renderer for screenshot')
      process.exit(1)
    }
    const text = renderer.getPaintedText?.() ?? []
    console.log('painted count', text.length)
    console.log('painted sample', text.slice(0, 12))
    renderer.captureScreenshot(screenshotPath)
    console.log('screenshot', screenshotPath)
    process.exit(0)
  }, 800)
}
