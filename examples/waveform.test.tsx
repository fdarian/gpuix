import fs from 'fs'
import path from 'path'
import { fileURLToPath } from 'url'
import { describe, expect, it } from 'vitest'
import React from 'react'
import { createTestRoot, hasNativeTestRenderer } from '@gpuix/react/testing'
import { WaveformApp } from './waveform'

const describeNative = hasNativeTestRenderer ? describe : describe.skip
const SHOTS = path.resolve(path.dirname(fileURLToPath(import.meta.url)), 'screenshots')
const isCI = !!process.env.CI

fs.mkdirSync(SHOTS, { recursive: true })

function similarity(a: Buffer, b: Buffer): number {
  const len = Math.max(a.length, b.length)
  if (len === 0) return 1
  let matching = 0
  for (let i = 0; i < len; i++) {
    if (a[i] === b[i]) matching++
  }
  return matching / len
}

describeNative('waveform example', () => {
  it('paints a waveform through setImagePixels', () => {
    const root = createTestRoot({ width: 800, height: 420 })
    const first = path.join(SHOTS, 'waveform-phase-0.png')
    const second = path.join(SHOTS, 'waveform-phase-1.png')
    if (fs.existsSync(first)) fs.unlinkSync(first)
    if (fs.existsSync(second)) fs.unlinkSync(second)

    root.render(<WaveformApp phase={0} />)
    root.renderer.flush()
    root.renderer.captureScreenshot(first)

    root.render(<WaveformApp phase={1.2} />)
    root.renderer.flush()
    root.renderer.captureScreenshot(second)

    expect(fs.statSync(second).size).toBeGreaterThan(0)
    if (!isCI) {
      expect(similarity(fs.readFileSync(first), fs.readFileSync(second))).toBeLessThan(0.99)
    }
  })

  it('scrolls the last clip into view from the host ref', () => {
    const root = createTestRoot({ width: 800, height: 420 })
    root.render(<WaveformApp />)

    const list = root.renderer.findByTestId('clip-list')!
    expect(root.renderer.getScrollOffset(list.id)).toEqual([0, 0])

    const jump = root.renderer.findByTestId('jump-outro')!
    const bounds = root.renderer.getElementBounds(jump.id)!
    root.renderer.nativeSimulateClick(
      bounds.x + bounds.width / 2,
      bounds.y + bounds.height / 2,
    )

    const offset = root.renderer.getScrollOffset(list.id)
    expect(offset).not.toBeNull()
    expect(offset![1]).toBeGreaterThan(0)
  })
})
